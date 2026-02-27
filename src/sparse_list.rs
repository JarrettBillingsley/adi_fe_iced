#![allow(missing_docs)]
#![allow(unused)]

///! Horrifying amalgamation of iced/list-widget-reloaded's List, and iced's Scrollable.
///! Scrollable list of items which are dynamically instantiated, whose indexes are ordered but
///! potentially sparse. Also supports jumping to top/bottom/arbitrary item indexes, but doesn't
///! really have a concept of a "scroll position" the way a real Scrollable does. Scrolling is
///! instead kind of always relative to the current position.

use iced_core::{
    self, Clipboard, Element, Layout, Length, Point, Rectangle, Shell,
    Size, Vector, Widget, InputMethod,
    event::{ Event }, layout, mouse, touch, overlay, renderer, window,
    widget::{ self, operation, tree::{ self, Tree } },
};
use iced::keyboard;
use iced::widget::scrollable::{ RelativeOffset, AbsoluteOffset };

use std::time::{ Instant, Duration };
use std::cell::{ Ref, RefMut };
use std::collections::VecDeque;
use std::collections::BTreeMap;

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

    // passing the element in and returning it seems silly but BORROW CHECKER REASONS (well,
    // really, "lack of impl Borrow for &mut Element" reasons but same diff)
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

    fn item_changed(&mut self, state: &mut State, _renderer: &Renderer, idx: usize) {
        println!("processing changed item at {}", idx);

        // let size = self.layout_element(&state.last_limits, renderer, 0.0, self.new_element(idx))
        //     .2.size();

        // it may seem wasteful to clear the visible layouts, buuuuuuut the SparseList widget itself
        // is usually recreated every time these methods are called, meaning visible_elements is
        // empty, meaning visible_layouts needs to be recomputed anyway.
        state.visible_layouts.clear();
    }

    fn item_removed(&mut self, state: &mut State, idx: usize) {
        println!("processing removal of item at {}", idx);

        state.visible_layouts.clear();
    }

    fn item_added(&mut self, state: &mut State, _renderer: &Renderer, idx: usize) {
        println!("processing newly-added item at {}", idx);
        // compute the size
        // let size = self.layout_element(&state.last_limits, renderer, 0.0, self.new_element(idx))
        //     .2.size();

        state.visible_layouts.clear();
    }

    fn refresh(&mut self, state: &mut State, renderer: &Renderer, bounds: Rectangle) -> Vector {
        // ------------------------------------------------------------
        // setup

        // wipe out old state
        self.visible_elements.clear();
        state.visible_layouts.clear();

        // ------------------------------------------------------------
        // initial element (the one they asked to jump to)

        // grab the new position and offset. offset_y is where (measured from the top of the view)
        // the new element should be positioned.
        let NewPosition { idx: initial_idx, offset_y } =
            std::mem::take(&mut state.new_position).expect("hey, YOU called refresh");

        // we need to first create the element from the item at initial_idx. its Y position doesn't
        // matter because we'll be computing it later from the heights of the items above it.
        let initial_element = self.new_element(initial_idx);
        let (initial_element, initial_tree, initial_layout) =
            self.layout_element(&state.last_limits, renderer, 0.0, initial_element);

        let mut visible_layouts = BTreeMap::from([(initial_idx, (initial_layout, initial_tree))]);
        let mut visible_elements = vec![initial_element];

        // ------------------------------------------------------------
        // if needed, we need to create elements before it until either those items go offscreen or
        // we run out, whichever comes first.

        // this is how many pixels above the initial element we need to fill in with elements.
        let mut pixels_remaining = offset_y;

        for (idx, item) in self.content.items_before(initial_idx) {
            if pixels_remaining <= 0.0 {
                // once we've filled all the pixels above this item, bail.
                break;
            }

            // first create and lay it out. again its Y position doesn't matter since we'll be
            // computing it in the next loop.
            let element = self.new_element_with(idx, item);
            let (element, tree, layout) = self.layout_element(&state.last_limits,
                renderer, 0.0, element);

            // decrement the number of pixels remaining based on its height
            pixels_remaining -= layout.size().height;

            // then push the things
            visible_layouts.insert(idx, (layout, tree));
            visible_elements.push(element);
        }

        // since we pushed them, they're in reverse order.
        visible_elements.reverse();

        // now recompute their Y positions so that they start at Y = 0.
        let mut current_y = 0.0;

        for (_, (layout, _)) in visible_layouts.iter_mut() {
            layout.move_to_mut((0.0, current_y));
            current_y += layout.size().height;
        }

        // ------------------------------------------------------------
        // THEN... we might need to create elements *after* the initial element.

        let initial_bounds = visible_layouts.last_key_value().unwrap().1.0.bounds();
        let mut current_y = initial_bounds.y + initial_bounds.height;

        // this may be an over-estimation in the case that there were not enough items to use up
        // the pixels_remaining in the first loop - but meh whatever
        let mut pixels_remaining = bounds.height - (offset_y + initial_bounds.height);

        for (idx, item) in self.content.items_after(initial_idx) {
            if pixels_remaining <= 0.0 {
                // once we've filled all the pixels below the initial item, bail.
                break;
            }

            // create and lay it out (and this time we know the right Y coordinate)
            let element = self.new_element_with(idx, item);
            let (element, tree, layout) = self.layout_element(&state.last_limits,
                renderer, current_y, element);

            // move current_y down
            current_y += layout.size().height;
            pixels_remaining -= layout.size().height;

            // and push the things
            visible_layouts.insert(idx, (layout, tree));
            visible_elements.push(element);
        }

        // note that we may have run out of items to fill up the view, but that's okay. in that case
        // it just won't be scrollable.

        // compute the total content size from the elements' layouts.
        let mut current_y: f32 = 0.0;

        for (_, (layout, _)) in visible_layouts.iter() {
            let bounds = layout.bounds();
            assert!(current_y == bounds.y); // should be true...
            current_y += bounds.height;
        }

        // done with everything, put it all in the state
        state.visible_layouts = visible_layouts;
        self.visible_elements = visible_elements;
        state.size = Size::new(bounds.width, current_y);

        // for scrolling, reset the state's scrolling offset and return the desired delta.
        state.offset_y = Offset::Absolute(0.0);
        Vector::new(0.0, state.offset_of(initial_idx) - offset_y)
    }

    /// about to scroll by `delta`; check if we need to manifest new items in the direction of
    /// scrolling (and delete old ones that fall off the other end). returns the adjusted delta
    /// to be passed to state.scroll()
    fn try_scroll(&mut self, state: &mut State, _renderer: &Renderer, view_bounds: Rectangle,
    delta: Vector) -> Vector {
        let content_bounds = Rectangle::with_size(state.size);

        // if the content is smaller than the view, then there's nothing to do - we can't scroll
        // anyway. same if there are no visible elements (empty list), or if delta is literally 0.
        if  content_bounds.height <= view_bounds.height ||
            self.visible_elements.is_empty() ||
            delta.y == 0.0 {
            return delta;
        }

        // negative delta is scrolling up, positive delta is scrolling down.

        // compute current and desired scroll offsets.
        let cur_offset_y = state.offset_y.absolute(view_bounds.height, content_bounds.height);
        let new_offset_y = cur_offset_y + delta.y;
        let new_view_bounds = view_bounds + delta;

        if new_offset_y < 0.0 {
            // scrolling up past top
            println!("scrolling off top!!");
        } else if new_offset_y > content_bounds.height - view_bounds.height {
            // scrolling down past bottom
            println!("scrolling off bottom!!");

            // need to append more items, if possible

        } else {
            // still within the content view, nothing to do
            println!("safe...");
        }

        delta
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Offset {
    Absolute(f32),
    Relative(f32),
}

impl Offset {
    fn absolute(self, viewport: f32, content: f32) -> f32 {
        match self {
            Offset::Absolute(absolute) => absolute.min((content - viewport).max(0.0)),
            Offset::Relative(percentage) => ((content - viewport) * percentage).max(0.0),
        }
    }

    fn translation(self, viewport: f32, content: f32) -> f32 {
        self.absolute(viewport, content)
    }
}

/// The current [`Viewport`] of the [`Scrollable`].
#[derive(Debug, Clone, Copy)]
pub struct Viewport {
    offset_y: Offset,
    bounds: Rectangle,
    content_bounds: Rectangle,
}

impl Viewport {
    /// Returns the [`AbsoluteOffset`] of the current [`Viewport`].
    pub fn absolute_offset(&self) -> AbsoluteOffset {
        let y = self
            .offset_y
            .absolute(self.bounds.height, self.content_bounds.height);

        AbsoluteOffset { x: 0.0, y }
    }

    /// Returns the [`RelativeOffset`] of the current [`Viewport`].
    pub fn relative_offset(&self) -> RelativeOffset {
        let AbsoluteOffset { x: _, y } = self.absolute_offset();

        let y = y / (self.content_bounds.height - self.bounds.height);

        RelativeOffset { x: 0.0, y }
    }

    /// Returns the bounds of the current [`Viewport`].
    pub fn bounds(&self) -> Rectangle {
        self.bounds
    }

    /// Returns the content bounds of the current [`Viewport`].
    pub fn content_bounds(&self) -> Rectangle {
        self.content_bounds
    }
}

fn notify_scroll<Message>(
    state: &mut State,
    bounds: Rectangle,
    content_bounds: Rectangle,
    shell: &mut Shell<'_, Message>,
) -> bool {
    if notify_viewport(state, bounds, content_bounds, shell) {
        state.last_scrolled = Some(Instant::now());
        true
    } else {
        false
    }
}

fn notify_viewport<Message>(
    state: &mut State,
    bounds: Rectangle,
    content_bounds: Rectangle,
    _shell: &mut Shell<'_, Message>,
) -> bool {
    if content_bounds.width <= bounds.width && content_bounds.height <= bounds.height {
        return false;
    }

    let viewport = Viewport {
        offset_y: state.offset_y,
        bounds,
        content_bounds,
    };

    // Don't publish redundant viewports to shell
    if let Some(last_notified) = state.last_notified {
        let last_relative_offset = last_notified.relative_offset();
        let current_relative_offset = viewport.relative_offset();

        let last_absolute_offset = last_notified.absolute_offset();
        let current_absolute_offset = viewport.absolute_offset();

        let unchanged =
            |a: f32, b: f32| (a - b).abs() <= f32::EPSILON || (a.is_nan() && b.is_nan());

        if last_notified.bounds == bounds
            && last_notified.content_bounds == content_bounds
            && unchanged(last_relative_offset.x, current_relative_offset.x)
            && unchanged(last_relative_offset.y, current_relative_offset.y)
            && unchanged(last_absolute_offset.x, current_absolute_offset.x)
            && unchanged(last_absolute_offset.y, current_absolute_offset.y)
        {
            return false;
        }
    }

    state.last_notified = Some(viewport);
    true
}

#[derive(Clone, Copy)]
struct NewPosition {
    idx:      usize,
    offset_y: f32,
}

struct State {
    last_limits:        layout::Limits,
    visible_layouts:    BTreeMap<usize, (layout::Node, Tree)>,
    size:               Size,
    visible_outdated:   bool,

    // scrolling stuff
    offset_y:           Offset,
    keyboard_modifiers: keyboard::Modifiers,
    last_scrolled:      Option<Instant>,
    last_notified:      Option<Viewport>,
    new_position:       Option<NewPosition>,
}

impl operation::Scrollable for State {
    fn snap_to(&mut self, offset: RelativeOffset<Option<f32>>) {
        State::snap_to(self, offset);
    }

    fn scroll_to(&mut self, offset: AbsoluteOffset<Option<f32>>) {
        State::scroll_to(self, offset);
    }

    fn scroll_by(&mut self, offset: AbsoluteOffset, bounds: Rectangle, content_bounds: Rectangle) {
        State::scroll_by(self, offset, bounds, content_bounds);
    }
}

impl State {
    fn needs_refresh(&self) -> bool {
        self.new_position.is_some()
    }

    fn offset_of(&self, idx: usize) -> f32 {
        self.visible_layouts.get(&idx).unwrap().0.bounds().y
    }

    fn height_of(&self, idx: usize) -> f32 {
        self.visible_layouts.get(&idx).unwrap().0.bounds().height
    }

    fn first_visible_index(&self) -> Option<usize> {
        self.visible_layouts.first_key_value().map(|(idx, _)| *idx)
    }

    fn update_limits(&mut self, loose_limits: layout::Limits) {
        if self.last_limits != loose_limits {
            // the original implementation just completely wiped out everything here, forcing a
            // recomputation of every single item in the list, and I'm not really sure why.
            self.last_limits = loose_limits;
        }
    }

    // Scrolling stuff
    fn scroll(&mut self, delta: Vector<f32>, bounds: Rectangle, content_bounds: Rectangle) {
        if bounds.height < content_bounds.height {
            self.offset_y = Offset::Absolute(
                (self.offset_y.absolute(bounds.height, content_bounds.height) + delta.y)
                    .clamp(0.0, content_bounds.height - bounds.height),
            );

            println!(":::::::::: scroll - State::scroll(), offset_y = {:?}", self.offset_y);
        } else {
            println!(":::::::::: scroll - State::scroll() didn't scroll, bounds = {:?}, \
                content_bounds = {:?}", bounds, content_bounds);
        }
    }

    fn scroll_and_notify<Message>(&mut self, delta: Vector<f32>, bounds: Rectangle,
    content_bounds: Rectangle, shell: &mut Shell<'_, Message>) {
        self.scroll(delta, bounds, content_bounds);

        let has_scrolled = notify_scroll(self, bounds, content_bounds, shell);

        if has_scrolled || self.last_scrolled.is_some() {
            shell.capture_event();
        }
    }

    fn snap_to(&mut self, offset: RelativeOffset<Option<f32>>) {
        if let Some(y) = offset.y {
            self.offset_y = Offset::Relative(y.clamp(0.0, 1.0));
            println!(":::::::::: scroll - State::snap_to(), offset_y = {:?}", self.offset_y);
        }
    }

    fn scroll_to(&mut self, offset: AbsoluteOffset<Option<f32>>) {
        if let Some(y) = offset.y {
            self.offset_y = Offset::Absolute(y.max(0.0));
            println!(":::::::::: scroll - State::scroll_to(), offset_y = {:?}", self.offset_y);
        }
    }

    fn scroll_by(&mut self, offset: AbsoluteOffset, bounds: Rectangle, content_bounds: Rectangle) {
        self.scroll(Vector::new(offset.x, offset.y), bounds, content_bounds);
    }

    /// Returns the scrolling translation of the [`State`], given
    /// the bounds of the [`Scrollable`] and its contents.
    fn translation(
        &self,
        bounds: Rectangle,
        content_bounds: Rectangle,
    ) -> Vector {
        Vector::new(
            0.0,
            self.offset_y
                .translation(bounds.height, content_bounds.height)
                .round()
        )
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
        let new_position =
            self.content.items_after(self.content.first().unwrap())
            .nth(9).map(|(idx, _)| NewPosition { idx, offset_y: 10.0 });

        tree::State::new(State {
            last_limits:      layout::Limits::NONE,
            visible_layouts:  BTreeMap::new(),
            size:             Size::ZERO,
            visible_outdated: false,
            new_position,

            offset_y: Offset::Absolute(0.0),
            keyboard_modifiers: keyboard::Modifiers::default(),
            last_scrolled: None,
            last_notified: None
        })
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Fill,
        }
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits)
    -> layout::Node {
        let state = tree.state.downcast_mut::<State>();
        state.update_limits(limits.loose());

        if !state.needs_refresh() {
            let mut changes = self.content.changes_mut();

            while let Some(change) = changes.pop_front() {
                match change {
                    Change::Changed { idx } => self.item_changed(state, renderer, idx),
                    Change::Removed { idx } => self.item_removed(state, idx),
                    Change::Added   { idx } => self.item_added  (state, renderer, idx),
                }
            }
        }

        layout::Node::new(limits.resolve(Length::Fill, Length::Fill, state.size))
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
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();

        let bounds = layout.bounds();
        let cursor_over_scrollable = cursor.position_over(bounds);
        let mut content_bounds = Rectangle::with_size(state.size);

        let last_offset_y = state.offset_y;

        // 1. check if it's been long enough to stop scrolling or they moved the mouse or whatever
        if let Some(last_scrolled) = state.last_scrolled {
            let clear_transaction = match event {
                Event::Mouse(
                    mouse::Event::ButtonPressed(_)
                    | mouse::Event::ButtonReleased(_)
                    | mouse::Event::CursorLeft,
                ) => true,
                Event::Mouse(mouse::Event::CursorMoved { .. }) =>
                     last_scrolled.elapsed() > Duration::from_millis(100),
                _ => last_scrolled.elapsed() > Duration::from_millis(1500),
            };

            if clear_transaction {
                println!(":::::::::: scroll - clear transaction");
                state.last_scrolled = None;
            }
        }

        let offset = layout.position() - Point::ORIGIN;

        if state.last_scrolled.is_none()
            || !matches!(event, Event::Mouse(mouse::Event::WheelScrolled { .. }))
        {
            let translation = state.translation(bounds, content_bounds);

            let cursor = match cursor_over_scrollable {
                Some(cursor_position) => mouse::Cursor::Available(cursor_position + translation),
                _                     => cursor.levitate() + translation,
            };

            let had_input_method = shell.input_method().is_enabled();

            // println!(":::::::::: scroll - forwarding event");

            for (element, (_index, (layout, tree))) in
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
                    &Rectangle {
                        x: bounds.x + translation.x,
                        y: bounds.y + translation.y,
                        ..bounds
                    },
                )
            }

            if !had_input_method
                && let InputMethod::Enabled { cursor, .. } = shell.input_method_mut()
            {
                *cursor = *cursor - translation;
            }
        }

        // if they let go of the mouse/finger, return.
        if matches!(
            event,
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                | Event::Touch(
                    touch::Event::FingerLifted { .. } | touch::Event::FingerLost { .. }
                )
        ) {
            println!(":::::::::: scroll - let go");
            return;
        }

        // if the event was already captured, return.
        if shell.is_event_captured() {
            println!(":::::::::: scroll - event captured");
            return;
        }

        match event {
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if cursor_over_scrollable.is_none() {
                    println!(":::::::::: scroll - cursor not over scrollable");
                    return;
                }

                // if they used the mouse wheel over the viewport, SCROLL IT!
                let delta = match *delta {
                    mouse::ScrollDelta::Lines { x, y } => {
                        let is_shift_pressed = state.keyboard_modifiers.shift();

                        // macOS automatically inverts the axes when Shift is pressed
                        let (x, y) = if cfg!(target_os = "macos") && is_shift_pressed {
                            (y, x)
                        } else {
                            (x, y)
                        };

                        let movement = if !is_shift_pressed {
                            Vector::new(x, y)
                        } else {
                            Vector::new(y, x)
                        };

                        // TODO: Configurable speed/friction (?)
                        -movement * 60.0
                    }
                    mouse::ScrollDelta::Pixels { x, y } => -Vector::new(x, y),
                };

                let delta = self.try_scroll(state, renderer, bounds, delta);
                content_bounds = Rectangle::with_size(state.size);
                println!(":::::::::: scroll - scrolling by {:?}", delta);
                state.scroll_and_notify(delta, bounds, content_bounds, shell);
            }
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                // check for keyboard modifiers (to undo shift-scroll axis-swapping on macos)
                println!(":::::::::: scroll - capturing keyboard modifiers");
                state.keyboard_modifiers = *modifiers;
            }
            Event::Window(window::Event::RedrawRequested(_)) => {
                println!(":::::::::: scroll - redraw requested");

                if state.needs_refresh() {
                    let delta = self.refresh(state, renderer, bounds);
                    content_bounds = Rectangle::with_size(state.size);
                    println!(":::::::::: scroll - refreshed! scrolling by {:?}", delta);
                    state.scroll_and_notify(delta, bounds, content_bounds, shell);
                }

                /*
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
                */
            }

            _ => {}
        }

        if last_offset_y != state.offset_y {
            println!(":::::::::: scroll - offset changed, requesting redraw");
            shell.request_redraw();
        }
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

        let bounds = layout.bounds();
        let content_bounds = Rectangle::with_size(state.size);

        let Some(visible_bounds) = bounds.intersection(viewport) else {
            return;
        };

        let translation = state.translation(bounds, content_bounds);

        let cursor = match cursor.position_over(bounds) {
            Some(cursor_position) => mouse::Cursor::Available(cursor_position + translation),
            _                     => cursor.levitate() + translation,
        };

        let offset = layout.position() - Point::ORIGIN;

        renderer.with_layer(visible_bounds, |renderer| {
            renderer.with_translation(
                Vector::new(-translation.x, -translation.y),
                |renderer| {
                    for (element, (_item, (layout, tree))) in
                        self.visible_elements.iter().zip(&state.visible_layouts)
                    {
                        element.as_widget().draw(
                            tree,
                            renderer,
                            theme,
                            style,
                            Layout::with_offset(offset, layout),
                            cursor,
                            &Rectangle {
                                x: visible_bounds.x + translation.x,
                                y: visible_bounds.y + translation.y,
                                ..visible_bounds
                            },
                        );
                    }
                },
            );
        });
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
            .map(|(element, (_item, (layout, tree)))| {
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

        // TODO: this needed? maybe to programmatically scroll? need an id for that
        // let bounds = layout.bounds();
        // let content_layout = layout.children().next().unwrap();
        // let content_bounds = content_layout.bounds();
        // let translation = state.translation(bounds, content_bounds);

        // operation.scrollable(self.id.as_ref(), bounds, content_bounds, translation, state);

        let offset = layout.position() - Point::ORIGIN;

        for (element, (_item, (layout, tree))) in
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
            .filter_map(|(child, (_item, (layout, tree)))| {
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
    /// return an iterator over the items before (and not including) the item at `idx`. The items
    /// should be given in reverse order!
    fn items_before(&'a self, idx: usize)
        -> Box<dyn DoubleEndedIterator<Item = (usize, &'a V)> + 'a>;

    /// return an iterator over the items after (and not including) the item at `idx`
    fn items_after(&'a self, idx: usize)
        -> Box<dyn DoubleEndedIterator<Item = (usize, &'a V)> + 'a>;

    // TODO: not needed?
    /// return an iterator over the items before and including the item at `idx`. The items should
    /// be given in reverse order!
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