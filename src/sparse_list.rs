#![allow(missing_docs)]
///! Taken from iced/list-widget-reloaded

use iced_core::event::{Event};
use iced_core::layout;
use iced_core::mouse;
use iced_core::overlay;
use iced_core::renderer;
use iced_core::widget;
use iced_core::widget::tree::{self, Tree};
use iced_core::window;
use iced_core::{
    self, Clipboard, Element, Layout, Length, Point, Rectangle, Shell,
    Size, Vector, Widget,
};

use std::cell::{ Ref, RefMut };
use std::cmp::Ordering;
use std::collections::VecDeque;
use std::collections::BTreeMap;
use std::ops::Bound;

/// Creates a new [`SparseList`] with the provided [`Content`] and
/// closure to view an item of the [`SparseList`].
///
/// [`SparseList`]: crate::SparseList
/// [`Content`]: crate::sparse_list::Content
pub fn sparse_list<'a, T, Message, Theme, Renderer: iced_core::Renderer>(
    content: &'a dyn IContent<'a, T>,
    view_item: impl Fn(usize, &'a T) -> Element<'a, Message, Theme, Renderer> + 'a,
) -> SparseList<'a, T, Message, Theme, Renderer> {
    SparseList::new(content, view_item)
}

#[allow(missing_debug_implementations)]
pub struct SparseList<'a, T, Message, Theme, Renderer> {
    content: &'a dyn IContent<'a, T>,
    view_item:
        Box<dyn Fn(usize, &'a T) -> Element<'a, Message, Theme, Renderer> + 'a>,
    visible_elements: Vec<Element<'a, Message, Theme, Renderer>>,
}

impl<'a, T, Message, Theme, Renderer: iced_core::Renderer>
    SparseList<'a, T, Message, Theme, Renderer> {
    pub fn new(
        content: &'a dyn IContent<'a, T>,
        view_item: impl Fn(usize, &'a T) -> Element<'a, Message, Theme, Renderer>
            + 'a,
    ) -> Self {
        Self {
            content,
            view_item: Box::new(view_item),
            visible_elements: Vec::new(),
        }
    }

    fn new_element(&self, idx: usize) -> Element<'a, Message, Theme, Renderer> {
        self.new_element_with(idx, &self.content.get(idx).unwrap())
    }

    fn new_element_with(&self, idx: usize, item: &'a T) -> Element<'a, Message, Theme, Renderer> {
        (self.view_item)(idx, item)
    }

    // passing the element in and returning it seems silly but BORROW CHECKER REASONS
    // well, really, "lack of impl Borrow for &mut Element" reasons but same idea
    fn layout_element(&self, limits: &layout::Limits, renderer: &Renderer, y: f32,
    mut element: Element<'a, Message, Theme, Renderer>)
    -> (Element<'a, Message, Theme, Renderer>, Tree, layout::Node) {
        let mut tree = Tree::new(&element);

        let layout = element
            .as_widget_mut()
            .layout(&mut tree, renderer, limits)
            .move_to((0.0, y));

        (element, tree, layout)
    }

    fn item_changed(&mut self, state: &mut State, renderer: &Renderer, idx: usize) {
        println!("processing changed item at {}", idx);

        let size = self.layout_element(&state.last_limits, renderer, 0.0, self.new_element(idx))
            .2.size();

        state.set_width(idx, size.width);
        state.set_offset(idx, size.height);

        // it may seem wasteful to clear the visible layouts, buuuuuuut the SparseList widget itself
        // is usually recreated every time these methods are called, meaning visible_elements is
        // empty, meaning visible_layouts needs to be recomputed anyway.
        state.visible_layouts.clear();
    }

    fn item_removed(&mut self, state: &mut State, idx: usize) {
        println!("processing removal of item at {}", idx);

        state.remove_width(idx);
        state.remove_offset(idx);
        state.visible_layouts.clear();
    }

    fn item_added(&mut self, state: &mut State, renderer: &Renderer, idx: usize) {
        println!("processing newly-added item at {}", idx);
        // compute the size
        let size = self.layout_element(&state.last_limits, renderer, 0.0, self.new_element(idx))
            .2.size();

        state.set_width(idx, size.width);
        state.set_offset(idx, size.height);
        state.visible_layouts.clear();
    }

    fn update_task(&mut self, state: &mut State, renderer: &Renderer) {
        match &mut state.task {
            Task::Idle => {}
            Task::Computing {
                current,
                size,
                widths,
                offsets,
            } => {
                const MAX_BATCH_SIZE: usize = 50;

                let batch = self.content.items_at_and_after(*current).take(MAX_BATCH_SIZE);

                let mut max_width = size.width;
                let mut accumulated_height = offsets.last_key_value()
                    .map(|(_, v)| *v).unwrap_or(0.0);
                let mut last_idx = *current;

                for (idx, item) in batch {
                    let size = self.layout_element(&state.last_limits, renderer, accumulated_height,
                        self.new_element_with(idx, &item)).2.size();

                    max_width = max_width.max(size.width);

                    offsets.insert(idx, accumulated_height);
                    widths.insert(idx, size.width);

                    accumulated_height += size.height;
                    last_idx = idx;
                }

                *size = Size::new(max_width, accumulated_height);

                match self.content.items_after(last_idx).next() {
                    Some((rest_idx, _)) if rest_idx < self.content.domain() => {
                        *current = rest_idx;
                    }

                    _ => {
                        offsets.insert(self.content.domain(), accumulated_height);
                        state.offsets = std::mem::take(offsets);
                        state.widths = std::mem::take(widths);
                        state.size = std::mem::take(size);
                        state.task = Task::Idle;
                    }
                }
            }
        }
    }
}

enum Task {
    Idle,
    Computing {
        current:     usize,
        offsets:     BTreeMap<usize, f32>,
        widths:      BTreeMap<usize, f32>,
        size:        Size,
    },
}

impl Task {
    fn is_computing(&self) -> bool {
        matches!(self, Task::Computing { .. })
    }
}

struct State {
    last_limits:      layout::Limits,
    visible_layouts:  Vec<(usize, layout::Node, Tree)>,
    size:             Size,
    offsets:          BTreeMap<usize, f32>,
    widths:           BTreeMap<usize, f32>,
    task:             Task,
    visible_outdated: bool,
    is_new:           bool,
}

impl State {
    fn recompute(&mut self) {
        self.task = Task::Computing {
            current:     0,
            offsets:     BTreeMap::new(),
            widths:      BTreeMap::new(),
            size:        Size::ZERO,
        };
        self.visible_layouts.clear();
        self.is_new = false;
    }

    fn recompute_if_new(&mut self) {
        if self.is_new {
            self.recompute();
        }
    }

    fn offset_of(&self, idx: usize) -> f32 {
        *self.offsets.get(&idx).unwrap()
    }

    fn offset_after(&self, idx: usize) -> f32 {
        *self.offsets.range((Bound::Excluded(idx), Bound::Unbounded))
            .next().expect("no item after").1
    }

    fn height_of(&self, idx: usize) -> f32 {
        self.offset_after(idx) - self.offset_of(idx)
    }

    fn offsets_after_mut(&mut self, idx: usize) -> impl Iterator<Item = &mut f32> {
        self.offsets.range_mut((Bound::Excluded(idx), Bound::Unbounded)).map(|(_, o)| o)
    }

    fn slide_offsets_after(&mut self, idx: usize, delta: f32) {
        for offset in self.offsets_after_mut(idx) {
            *offset += delta;
        }

        self.size.height += delta;
    }

    /// removes an offset at the given index. automatically determines the item's height based on
    /// existing items and moves the offsets of all items after it up.
    fn remove_offset(&mut self, idx: usize) {
        let height = self.height_of(idx);
        self.offsets.remove(&idx).expect("removed an offset that didn't exist");
        self.slide_offsets_after(idx, -height);
    }

    fn first_visible_index(&self) -> Option<usize> {
        self.visible_layouts.first().map(|(idx, _, _)| *idx)
    }

    /// adds or changes an offset at the given index to the given height. automatically determines
    /// the new item's offset based on existing items and updates the offsets of all items after
    /// it.
    fn set_offset(&mut self, idx: usize, new_height: f32) {
        // find offset of item right after idx
        let offset_after = self.offset_after(idx);

        let delta = if let Some(cur_offset) = self.offsets.get(&idx) {
            let old_height = offset_after - cur_offset;
            let height_difference = new_height - old_height;
            height_difference
        } else {
            self.offsets.insert(idx, offset_after);
            new_height
        };

        self.slide_offsets_after(idx, delta);
    }

    // sets the width of a new or existing item.
    fn set_width(&mut self, idx: usize, new_width: f32) {
        if let Some(original_width) = self.widths.insert(idx, new_width) {
            if new_width < original_width {
                self.minimize_width(original_width);
                return;
            }
        }

        self.size.width = self.size.width.max(new_width);
    }

    fn remove_width(&mut self, idx: usize) {
        let original_width = self.widths.remove(&idx)
            .expect("width should have been there");

        self.minimize_width(original_width);
    }

    fn minimize_width(&mut self, original_width: f32) {
        if original_width == self.size.width {
            self.size.width = self.widths.values().fold(
                0.0,
                |current, candidate| {
                    current.max(*candidate)
                },
            );
        }
    }

    fn update_limits(&mut self, loose_limits: layout::Limits) {
        if self.last_limits != loose_limits {
            // the original implementation just completely wiped out everything here, forcing a
            // recomputation of every single item in the list, and I'm not really sure why.
            self.last_limits = loose_limits;
        }
    }
}

impl<'a, T, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for SparseList<'a, T, Message, Theme, Renderer>
where
    Renderer: iced_core::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            last_limits:      layout::Limits::NONE,
            visible_layouts:  Vec::new(),
            size:             Size::ZERO,
            offsets:          BTreeMap::new(),
            widths:           BTreeMap::new(),
            task:             Task::Idle,
            visible_outdated: false,
            is_new:           true,
        })
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Shrink,
            height: Length::Shrink,
        }
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits)
    -> layout::Node {
        let state = tree.state.downcast_mut::<State>();
        state.update_limits(limits.loose());

        let mut changes = self.content.changes_mut();

        match state.task {
            Task::Idle => {
                while let Some(change) = changes.pop_front() {
                    match change {
                        Change::Changed { idx } => self.item_changed(state, renderer, idx),
                        Change::Removed { idx } => self.item_removed(state, idx),
                        Change::Added   { idx } => self.item_added  (state, renderer, idx),
                    }
                }
            }
            Task::Computing { .. } => {
                if !changes.is_empty() {
                    // If changes happen during layout computation,
                    // we simply restart the computation
                    changes.clear();
                    state.recompute();
                }
            }
        }

        // Recompute if new
        state.recompute_if_new();
        self.update_task(state, renderer);
        layout::Node::new(limits.resolve(Length::Shrink, Length::Shrink, state.size))
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let offset = layout.position() - Point::ORIGIN;

        for (element, (_index, layout, tree)) in
            self.visible_elements.iter_mut().zip(&mut state.visible_layouts)
        {
            element.as_widget_mut().update(
                tree,
                event,
                Layout::with_offset(offset, layout),
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            )
        }

        if let Event::Window(window::Event::RedrawRequested(_)) = event {
            if state.task.is_computing() {
                shell.invalidate_layout();
                shell.request_redraw();
            }

            let view_top = viewport.y - offset.y;
            let view_bottom = view_top + viewport.height;

            // TODO: temporary
            let temp_offsets = state.offsets.iter().collect::<Vec<_>>();

            if temp_offsets.is_empty() {
                return;
            }

            let start_idx = match binary_search_with_index_by(&temp_offsets, |_i, (_, height)| {
                    (*height).partial_cmp(&view_top).unwrap_or(Ordering::Equal)
                }) {
                    Ok(i)  => *temp_offsets[i].0,
                    Err(i) => *temp_offsets[i.saturating_sub(1)].0,
                }
                .min(self.content.domain());

            let end_idx = match binary_search_with_index_by(&temp_offsets, |_i, (_, height)| {
                    (*height).partial_cmp(&view_bottom).unwrap_or(Ordering::Equal)
                }) {
                    Ok(i) | Err(i) => {
                        if i == temp_offsets.len() {
                            *temp_offsets[i.saturating_sub(1)].0
                        } else {
                            *temp_offsets[i].0
                        }
                    }
                }
                .min(self.content.domain());

            if state.visible_outdated
                || state.visible_layouts.len() != self.visible_elements.len()
            {
                self.visible_elements.clear();
                state.visible_outdated = false;
            }

            // If view was recreated, we repopulate the visible elements
            // out of the internal visible layouts
            if self.visible_elements.is_empty() {
                self.visible_elements = state
                    .visible_layouts
                    .iter()
                    .map(|(idx, _, _)| self.new_element(*idx))
                    .collect();
            }

            // Clear no longer visible elements
            let top = state
                .visible_layouts
                .iter()
                .take_while(|(idx, _, _)| *idx < start_idx)
                .count();

            let bottom = state
                .visible_layouts
                .iter()
                .rev()
                .take_while(|(idx, _, _)| *idx >= end_idx)
                .count();

            self.visible_elements.splice(..top, []);
            state.visible_layouts.splice(..top, []);

            self.visible_elements
                .splice(self.visible_elements.len() - bottom.., []);
            state.visible_layouts
                .splice(state.visible_layouts.len() - bottom.., []);

            // Prepend new visible elements
            if let Some(first_visible) = state.first_visible_index() {
                if start_idx < first_visible {
                    for (i, (idx, item)) in self.content.items_at_and_after(start_idx).enumerate() {
                        if idx >= first_visible {
                            break;
                        }

                        let element = self.new_element_with(idx, item);
                        let (element, tree, layout) = self.layout_element(&state.last_limits,
                            renderer, state.offset_of(idx), element);
                        state.visible_layouts.insert(i, (idx, layout, tree));
                        self.visible_elements.insert(i, element);
                    }
                }
            }

            // Append new visible elements
            let last_visible = state
                .visible_layouts
                .last()
                .map(|(idx, _, _)| *idx + 1)
                .unwrap_or(start_idx);

            if last_visible < end_idx {
                for (idx, item) in self.content.items_at_and_after(last_visible) {
                    if idx >= end_idx {
                        break;
                    }

                    let element = self.new_element_with(idx, item);
                    let (element, tree, layout) = self.layout_element(&state.last_limits,
                        renderer, state.offset_of(idx), element);
                    state.visible_layouts.push((idx, layout, tree));
                    self.visible_elements.push(element);
                }
            }
        }

        // status
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let offset = layout.position() - Point::ORIGIN;

        for (element, (_item, layout, tree)) in
            self.visible_elements.iter().zip(&state.visible_layouts)
        {
            element.as_widget().draw(
                tree,
                renderer,
                theme,
                style,
                Layout::with_offset(offset, layout),
                cursor,
                viewport,
            );
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        let offset = layout.position() - Point::ORIGIN;

        self.visible_elements
            .iter()
            .zip(&state.visible_layouts)
            .map(|(element, (_item, layout, tree))| {
                element.as_widget().mouse_interaction(
                    tree,
                    Layout::with_offset(offset, layout),
                    cursor,
                    viewport,
                    renderer,
                )
            })
            .max()
            .unwrap_or_default()
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let offset = layout.position() - Point::ORIGIN;

        for (element, (_item, layout, tree)) in
            self.visible_elements.iter_mut().zip(&mut state.visible_layouts)
        {
            element.as_widget_mut().operate(
                tree,
                Layout::with_offset(offset, layout),
                renderer,
                operation,
            );
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let state = tree.state.downcast_mut::<State>();
        let offset = layout.position() - Point::ORIGIN;

        let children = self
            .visible_elements
            .iter_mut()
            .zip(&mut state.visible_layouts)
            .filter_map(|(child, (_item, layout, tree))| {
                child.as_widget_mut().overlay(
                    tree,
                    Layout::with_offset(offset, layout),
                    renderer,
                    viewport,
                    translation,
                )
            })
            .collect::<Vec<_>>();

        (!children.is_empty())
            .then(|| overlay::Group::with_children(children).overlay())
    }
}

impl<'a, T, Message, Theme, Renderer>
    From<SparseList<'a, T, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: iced_core::Renderer + 'a,
{
    fn from(list: SparseList<'a, T, Message, Theme, Renderer>) -> Self {
        Self::new(list)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// item at `idx` was changed
    Changed { idx: usize },

    /// removed the item at `idx`
    Removed { idx: usize },

    /// added an item at `idx`
    Added   { idx: usize },
}

pub trait IContent<'a, V: 'a> {
    // TODO: not needed?
    /// how many items there are; always <= `domain()`
    fn len(&self) -> usize;

    // TODO: not needed?
    /// true if `self.len() == 0`
    fn is_empty(&self) -> bool { self.len() == 0 }

    /// the valid domain of indexes `[0 .. domain)`, but not every index need be present
    fn domain(&self) -> usize;

    // TODO: not needed?
    /// return the first valid index, or `None` if there are no valid indices
    fn first(&self) -> Option<usize>;

    // TODO: not needed?
    /// return the last valid index, or `None` if there are no valid indices
    fn last(&self) -> Option<usize>;

    /// get the item with the given index, if it exists
    fn get(&self, idx: usize) -> Option<&V>;

    // TODO: not needed?
    /// return an iterator over the items before (and not including) the item at `idx`
    fn items_before(&'a self, idx: usize)
        -> Box<dyn DoubleEndedIterator<Item = (usize, &'a V)> + 'a>;

    /// return an iterator over the items after (and not including) the item at `idx`
    fn items_after(&'a self, idx: usize)
        -> Box<dyn DoubleEndedIterator<Item = (usize, &'a V)> + 'a>;

    // TODO: not needed?
    /// return an iterator over the items before and including the item at `idx`
    fn items_at_and_before(&'a self, idx: usize)
        -> Box<dyn DoubleEndedIterator<Item = (usize, &'a V)> + 'a>;

    /// return an iterator over the items after and including the item at `idx`
    fn items_at_and_after(&'a self, idx: usize)
        -> Box<dyn DoubleEndedIterator<Item = (usize, &'a V)> + 'a>;

    // TODO: not needed?
    /// set the item at the given index to the given value. if `idx` was not present, returns
    /// `None`; otherwise, returns the old value at this index.
    fn insert(&mut self, idx: usize, val: V) -> Option<V>;

    // TODO: not needed?
    /// try to remove the item at the given index; returns `true` if there was an item there and
    /// `false` if there wasn't
    fn remove(&mut self, idx: usize) -> bool;

    // TODO: not needed?
    /// get the queue of changes which have occurred.
    fn changes(&'a self) -> Ref<'a, VecDeque<Change>>;

    /// same as above but mutable
    fn changes_mut(&'a self) -> RefMut<'a, VecDeque<Change>>;
}

/// SAFETY: Copied from the `std` library.
#[allow(unsafe_code)]
fn binary_search_with_index_by<'a, T, F>(
    slice: &'a [T],
    mut f: F,
) -> Result<usize, usize>
where
    F: FnMut(usize, &'a T) -> Ordering,
{
    use std::cmp::Ordering::*;

    // INVARIANTS:
    // - 0 <= left <= left + size = right <= self.len()
    // - f returns Less for everything in self[..left]
    // - f returns Greater for everything in self[right..]
    let mut size = slice.len();
    let mut left = 0;
    let mut right = size;
    while left < right {
        let mid = left + size / 2;

        // SAFETY: the while condition means `size` is strictly positive, so
        // `size/2 < size`. Thus `left + size/2 < left + size`, which
        // coupled with the `left + size <= self.len()` invariant means
        // we have `left + size/2 < self.len()`, and this is in-bounds.
        let cmp = f(mid, unsafe { slice.get_unchecked(mid) });

        // This control flow produces conditional moves, which results in
        // fewer branches and instructions than if/else or matching on
        // cmp::Ordering.
        // This is x86 asm for u8: https://rust.godbolt.org/z/698eYffTx.
        left = if cmp == Less { mid + 1 } else { left };
        right = if cmp == Greater { mid } else { right };
        if cmp == Equal {
            return Ok(mid);
        }

        size = right - left;
    }

    Err(left)
}
