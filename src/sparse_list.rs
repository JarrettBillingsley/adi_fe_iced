#![allow(missing_docs)]
///! Taken from iced/list-widget-reloaded

// use ordered_float::NotNan;

use iced_core::event::{Event};
use iced_core::layout;
use iced_core::mouse;
use iced_core::overlay;
use iced_core::renderer;
use iced_core::widget;
use iced_core::widget::tree::{self, Tree};
use iced_core::window;
use iced_core::{
    self, Clipboard, Element, Layout, Length, /*Pixels,*/ Point, Rectangle, Shell,
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
pub fn sparse_list<'a, T, Message, Theme, Renderer>(
    content: &'a dyn IContent<'a, T>,
    view_item: impl Fn(usize, &'a T) -> Element<'a, Message, Theme, Renderer> + 'a,
) -> SparseList<'a, T, Message, Theme, Renderer> {
    SparseList::new(content, view_item)
}

#[allow(missing_debug_implementations)]
pub struct SparseList<'a, T, Message, Theme, Renderer> {
    content: &'a dyn IContent<'a, T>,
    // spacing: f32,
    view_item:
        Box<dyn Fn(usize, &'a T) -> Element<'a, Message, Theme, Renderer> + 'a>,
    visible_elements: Vec<Element<'a, Message, Theme, Renderer>>,
}

impl<'a, T, Message, Theme, Renderer> SparseList<'a, T, Message, Theme, Renderer> {
    pub fn new(
        content: &'a dyn IContent<'a, T>,
        view_item: impl Fn(usize, &'a T) -> Element<'a, Message, Theme, Renderer>
            + 'a,
    ) -> Self {
        Self {
            content,
            // spacing: 0.0,
            view_item: Box::new(view_item),
            visible_elements: Vec::new(),
        }
    }

    // /// Sets the vertical spacing _between_ elements.
    // ///
    // /// Custom margins per element do not exist in iced. You should use this
    // /// method instead! While less flexible, it helps you keep spacing between
    // /// elements consistent.
    // pub fn spacing(mut self, amount: impl Into<Pixels>) -> Self {
    //     self.spacing = amount.into().0;
    //     self
    // }
}

enum Task {
    Idle,
    Computing {
        current:     usize,
        offsets:     BTreeMap<usize, f32>,
        // offsets_rev: BTreeMap<NotNan<f32>, usize>,
        widths:      BTreeMap<usize, f32>,
        size:        Size,
    },
}

struct State {
    last_limits:      layout::Limits,
    visible_layouts:  Vec<(usize, layout::Node, Tree)>,
    size:             Size,
    offsets:          BTreeMap<usize, f32>,
    // offsets_rev:      BTreeMap<NotNan<f32>, usize>,
    widths:           BTreeMap<usize, f32>,
    task:             Task,
    visible_outdated: bool,
    is_new:           bool,
}

// fn notnan<T: ordered_float::FloatCore>(v: T) -> NotNan<T> {
//     NotNan::new(v).unwrap()
// }

impl State {
    fn recompute(&mut self, _domain: usize) {
        self.task = Task::Computing {
            current:     0,
            offsets:     BTreeMap::new(),
            // offsets_rev: BTreeMap::from([(notnan(0.0), domain)]),
            widths:      BTreeMap::new(),
            size:        Size::ZERO,
        };
        self.visible_layouts.clear();
        self.is_new = false;
    }

    fn is_new(&self) -> bool {
        self.is_new
    }

    fn offset_of(&self, idx: usize) -> f32 {
        *self.offsets.get(&idx).unwrap()
    }

    fn offset_after(&self, idx: usize) -> f32 {
        *self.offsets.range((Bound::Excluded(idx), Bound::Unbounded))
            .next().expect("no item after").1
    }

    fn offsets_after_mut(&mut self, idx: usize) -> impl Iterator<Item = &mut f32> {
        self.offsets.range_mut((Bound::Excluded(idx), Bound::Unbounded))
            .map(|(_, o)| o)
    }

    fn remove_offset(&mut self, idx: usize) {
        self.offsets.remove(&idx).expect("removed an offset that didn't exist");
        // self.offsets_rev.remove(SOMETHING);
    }

    /// adds an offset at the given index with the given height. automatically determines the new
    /// item's offset based on existing items and updates the offsets of all items after it.
    fn add_offset(&mut self, idx: usize, height: f32) {
        // find offset of item right after idx
        let prev_offset =
            *self.offsets.range((Bound::Excluded(idx), Bound::Unbounded))
            .next()
            .expect("is idx >= domain??")
            .1;

        // idx must not exist in self.offsets already (cause that'd be a bug...)
        assert!(self.offsets.insert(idx, prev_offset).is_none());

        // but the existing offset SHOULD exist... we're rewriting its idx tho
        // self.offsets_rev.insert(notnan(offset), idx).expect("reverse offset should exist");

        // then add height to everything after idx
        for (_, offset) in self.offsets.range_mut((Bound::Excluded(idx), Bound::Unbounded)) {
            *offset += height;
        }
    }

    // adds `height_difference` to `state.size.height`, and recomputes the total width assuming
    // an item of size `original_width` was removed.
    fn change_size(&mut self, height_difference: f32, original_width: f32, new_width: f32) {
        self.size.height += height_difference;

        if new_width < original_width {
            if original_width == self.size.width {
                self.size.width = self.widths.values().fold(
                    0.0,
                    |current, candidate| {
                        current.max(*candidate)
                    },
                );
            }
        } else if new_width > original_width {
            self.size.width = self.size.width.max(new_width);
        }
        // else, do nothing - width didn't change
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
            // offsets_rev:      BTreeMap::from([(notnan(0.0), domain)]),
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

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<State>();
        let loose_limits = limits.loose();

        if state.last_limits != loose_limits {
            state.last_limits = loose_limits;
        }

        let mut changes = self.content.changes_mut();

        match state.task {
            Task::Idle => {
                while let Some(change) = changes.pop_front() {
                    match change {
                        NewChange::Changed { idx } => {
                            println!("processing changed item at {}", idx);

                            // See if the changed element is visible right now
                            let visible_index = state
                                .visible_layouts
                                .iter_mut()
                                .position(|(i, _, _)| *i == idx);

                            // Compute its new layout
                            let new_layout = {
                                // Create the new element
                                let mut new_element = (self.view_item)(
                                    idx,
                                    &self.content.get(idx).unwrap(),
                                );

                                let mut new_tree;

                                let tree = if let Some(visible_index) = visible_index {
                                    // If it's currently visible, diff its tree and mark
                                    // visible_outdated = true so the visible elements will be
                                    // rebuilt from the visible_layouts on the next redraw
                                    let (_, _, tree) = &mut state.visible_layouts[visible_index];
                                    tree.diff(&new_element);
                                    state.visible_outdated = true;
                                    tree
                                } else {
                                    // Otherwise, make a new tree for it
                                    new_tree = Tree::new(&new_element);
                                    &mut new_tree
                                };

                                new_element
                                    .as_widget_mut()
                                    .layout(tree, renderer, &state.last_limits)
                            };

                            let new_size = new_layout.size();

                            let height_difference = new_size.height
                                - (state.offset_after(idx) - state.offset_of(idx));

                            // Move everything after it up/down
                            for offset in state.offsets_after_mut(idx) {
                                *offset += height_difference;
                            }

                            // Update its width
                            let original_width = *state.widths.get(&idx).unwrap();
                            state.widths.insert(idx, new_size.width);

                            if let Some(visible_index) = visible_index {
                                // If it's visible, update the visible layout and the layouts of
                                // everything after it.
                                state.visible_layouts[visible_index].1 = new_layout;

                                for (i, layout, _) in &mut state.visible_layouts[visible_index..] {
                                    layout.move_to_mut((0.0, *state.offsets.get(i).unwrap()));
                                }
                            } else if let Some(first_visible) = state.visible_layouts.first() {
                                // Otherwise, if it's not visible but it's before the first visible
                                // item, update their layouts.
                                let first_visible_index = first_visible.0;
                                if idx < first_visible_index {
                                    for (i, layout, _) in &mut state.visible_layouts[..] {
                                        layout.move_to_mut((0.0, *state.offsets.get(i).unwrap()));
                                    }
                                }
                            }

                            state.change_size(height_difference, original_width, new_size.width);
                        }
                        NewChange::Removed { idx } => {
                            println!("processing removal of item at {}", idx);
                            // compute height of removed item
                            let height = state.offset_after(idx) - state.offset_of(idx);

                            // get and remove width of removed item
                            let original_width = state.widths.remove(&idx)
                                .expect("width should have been there");

                            // remove offset of removed item, and shift everything below it up
                            for offset in state.offsets_after_mut(idx) {
                                *offset -= height;
                            }

                            let _ = state.remove_offset(idx);

                            // TODO: Smarter visible layout partial updates
                            // clear out the visible layouts
                            state.visible_layouts.clear();

                            state.change_size(-height, original_width, 0.0);
                        }
                        NewChange::Added { idx } => {
                            println!("processing newly-added item at {}", idx);
                            // compute the size
                            let size = {
                                let mut new_element = (self.view_item)(
                                    idx,
                                    &self.content.get(idx).unwrap(),
                                );

                                let mut tree = Tree::new(&new_element);

                                let layout = new_element.as_widget_mut().layout(
                                    &mut tree,
                                    renderer,
                                    &state.last_limits,
                                );

                                layout.size()
                            };

                            // TODO: Smarter visible layout partial updates
                            // clear out the visible layouts
                            state.visible_layouts.clear();

                            // insert the width and new item's offset into the state
                            state.widths.insert(idx, size.width);
                            state.add_offset(idx, size.height);

                            // compute the total width and height
                            state.change_size(size.height, 0.0, size.width);
                        }
                    }
                }
            }
            Task::Computing { .. } => {
                if !changes.is_empty() {
                    // If changes happen during layout computation,
                    // we simply restart the computation
                    changes.clear();
                    state.recompute(self.content.domain());
                }
            }
        }

        // Recompute if new
        {
            if state.is_new() {
                state.recompute(self.content.domain());
            }
        }

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
                    let bounds = {
                        let mut element = (self.view_item)(idx, &item);
                        let mut tree = Tree::new(&element);

                        let layout = element
                            .as_widget_mut()
                            .layout(&mut tree, renderer, &state.last_limits)
                            .move_to((0.0, accumulated_height));

                        layout.bounds()
                    };

                    max_width = max_width.max(bounds.width);

                    offsets.insert(idx, accumulated_height);
                    widths.insert(idx, bounds.width);

                    accumulated_height += bounds.height;
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

        let intrinsic_size = Size::new(
            state.size.width,
            state.size.height
                // + self.content.domain().saturating_sub(1) as f32 * self.spacing,
        );

        let size =
            limits.resolve(Length::Shrink, Length::Shrink, intrinsic_size);

        layout::Node::new(size)
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
                Layout::with_offset(
                    offset + Vector::new(0.0, 0.0), // self.spacing * *index as f32),
                    layout,
                ),
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            )
        }

        if let Event::Window(window::Event::RedrawRequested(_)) = event {
            match &mut state.task {
                Task::Idle => {}
                Task::Computing { .. } => {
                    shell.invalidate_layout();
                    shell.request_redraw();
                }
            }

            let view_top = viewport.y - offset.y;
            let view_bottom = view_top + viewport.height;

            // TODO: temporary
            let temp_offsets = state.offsets.iter().collect::<Vec<_>>();

            if temp_offsets.is_empty() {
                return;
            }

            let start_idx = match binary_search_with_index_by(&temp_offsets, |_i, (_, height)| {
                (*height) // + i.saturating_sub(1) as f32 * self.spacing)
                    .partial_cmp(&view_top)
                    .unwrap_or(Ordering::Equal)
                }) {
                    Ok(i)  => *temp_offsets[i].0,
                    Err(i) => *temp_offsets[i.saturating_sub(1)].0,
                }
                .min(self.content.domain());

            let end_idx = match binary_search_with_index_by(&temp_offsets, |_i, (_, height)| {
                (*height) // + i.saturating_sub(1) as f32 * self.spacing)
                    .partial_cmp(&view_bottom)
                    .unwrap_or(Ordering::Equal)
                }) {
                    Ok(i) | Err(i) => {
                        if i == temp_offsets.len() {
                            *temp_offsets[i - 1].0
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
                    .map(|(idx, _, _)| {
                        (self.view_item)(*idx, &self.content.get(*idx).unwrap())
                    })
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

            let _ = self.visible_elements.splice(..top, []);
            let _ = state.visible_layouts.splice(..top, []);

            let _ = self
                .visible_elements
                .splice(self.visible_elements.len() - bottom.., []);
            let _ = state
                .visible_layouts
                .splice(state.visible_layouts.len() - bottom.., []);

            // Prepend new visible elements
            if let Some(first_visible) = state.visible_layouts.first().map(|(idx, _, _)| *idx) {
                if start_idx < first_visible {
                    for (i, (idx, item)) in self.content.items_at_and_after(start_idx).enumerate() {
                        if idx >= first_visible {
                            break;
                        }

                        let mut element = (self.view_item)(idx, item);
                        let mut tree = Tree::new(&element);

                        let layout = element
                            .as_widget_mut()
                            .layout(&mut tree, renderer, &state.last_limits)
                            .move_to((
                                0.0,
                                state.offset_of(idx)
                                    // + (start + i) as f32 * self.spacing,
                            ));

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

                    let mut element = (self.view_item)(idx, item);
                    let mut tree = Tree::new(&element);

                    let layout = element
                        .as_widget_mut()
                        .layout(&mut tree, renderer, &state.last_limits)
                        .move_to((
                            0.0,
                            state.offset_of(idx)
                                // + (last_visible + i) as f32 * self.spacing,
                        ));

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
pub enum NewChange {
    /// item at `idx` was changed
    Changed { idx: usize },

    /// removed the item at `idx`
    Removed { idx: usize },

    /// added an item at `idx`
    Added   { idx: usize },
}

pub trait IContent<'a, V: 'a> {
    /// how many items there are; always <= `domain()`
    fn len(&self) -> usize;

    /// true if `self.len() == 0`
    fn is_empty(&self) -> bool { self.len() == 0 }

    /// the valid domain of indexes `[0 .. domain)`, but not every index need be present
    fn domain(&self) -> usize;

    /// return the first valid index, or `None` if there are no valid indices
    fn first(&self) -> Option<usize>;

    /// return the last valid index, or `None` if there are no valid indices
    fn last(&self) -> Option<usize>;

    /// get the item with the given index, if it exists
    fn get(&self, idx: usize) -> Option<&V>;

    /// return an iterator over the items before (and not including) the item at `idx`
    fn items_before(&'a self, idx: usize)
        -> Box<dyn DoubleEndedIterator<Item = (usize, &'a V)> + 'a>;

    /// return an iterator over the items after (and not including) the item at `idx`
    fn items_after(&'a self, idx: usize)
        -> Box<dyn DoubleEndedIterator<Item = (usize, &'a V)> + 'a>;

    /// return an iterator over the items before and including the item at `idx`
    fn items_at_and_before(&'a self, idx: usize)
        -> Box<dyn DoubleEndedIterator<Item = (usize, &'a V)> + 'a>;

    /// return an iterator over the items after and including the item at `idx`
    fn items_at_and_after(&'a self, idx: usize)
        -> Box<dyn DoubleEndedIterator<Item = (usize, &'a V)> + 'a>;

    /// set the item at the given index to the given value. if `idx` was not present, returns
    /// `None`; otherwise, returns the old value at this index.
    fn insert(&mut self, idx: usize, val: V) -> Option<V>;

    /// try to remove the item at the given index; returns `true` if there was an item there and
    /// `false` if there wasn't
    fn remove(&mut self, idx: usize) -> bool;

    /// get the queue of changes which have occurred.
    fn changes(&'a self) -> Ref<'a, VecDeque<NewChange>>;

    /// same as above but mutable
    fn changes_mut(&'a self) -> RefMut<'a, VecDeque<NewChange>>;
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
