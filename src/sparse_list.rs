#![allow(missing_docs)]
// #![allow(unused)]

///! Horrifying amalgamation of iced/list-widget-reloaded's List, and iced's Scrollable.
///! Scrollable list of items which are dynamically instantiated, whose indexes are ordered but
///! potentially sparse. Also supports jumping to top/bottom/arbitrary item indexes, but doesn't
///! really have a concept of a "scroll position" the way a real Scrollable does. Scrolling is
///! instead kind of always relative to the current position.

use std::ops::{ Bound };
use std::time::{ Instant, Duration };
use std::collections::{ BTreeMap, VecDeque };

use iced_core::{
	self, Clipboard, Element, Layout, Length, Point, Rectangle, Shell,
	Size, Vector, Widget, InputMethod,
	event::{ Event }, layout, mouse, touch, overlay, renderer, window,
	widget::{ self, operation, tree::{ self, Tree } },
};
use iced::keyboard;
use iced::widget::scrollable::{ RelativeOffset, AbsoluteOffset };

// ------------------------------------------------------------------------------------------------
// Why isn't this method on Rectangle?
// ------------------------------------------------------------------------------------------------

trait RectangleEx {
	fn bottom(&self) -> f32;
}

impl RectangleEx for Rectangle<f32> {
	fn bottom(&self) -> f32 {
		self.y + self.height
	}
}

// ------------------------------------------------------------------------------------------------
// User-implemented interface
// ------------------------------------------------------------------------------------------------

/// Some kind of change which occurred in a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
	/// changed the item at `idx`
	Changed { idx: usize },

	/// removed the item at `idx`
	Removed { idx: usize },

	/// added an item at `idx`
	Added   { idx: usize },
}

/// The interface to the underlying content which you must implement for `SparseList` to be able to
/// display your items.
pub trait IContent<'a, V: 'a> {
	/// how many items there are; always <= `domain()`
	fn len(&self) -> usize;

	/// true if `self.len() == 0`
	fn is_empty(&self) -> bool { self.len() == 0 }

	// // TODO: not needed?
	// /// the valid domain of indexes `[0 .. domain)`, but not every index need be present
	// fn domain(&self) -> usize;

	/// return the first valid index, or `None` if there are no valid indices
	fn first(&self) -> Option<usize>;

	/// return the last valid index, or `None` if there are no valid indices
	fn last(&self) -> Option<usize>;

	/// get the item with the given index, if it exists
	fn get(&self, idx: usize) -> Option<&V>;

	/// return an iterator over the items before (and not including) the item at `idx`. The items
	/// should be given in reverse order!
	fn items_before(&'a self, idx: usize)
		-> Box<dyn DoubleEndedIterator<Item = (usize, &'a V)> + 'a>;

	/// return an iterator over the items after (and not including) the item at `idx`
	fn items_after(&'a self, idx: usize)
		-> Box<dyn DoubleEndedIterator<Item = (usize, &'a V)> + 'a>;

	/// get a mutable queue of changes which have occurred.
	fn changes(&self) -> VecDeque<Change>;
}

// ------------------------------------------------------------------------------------------------
// SparseList
// ------------------------------------------------------------------------------------------------

pub fn sparse_list<'a, T, Message, Theme, Renderer: iced_core::Renderer>(
	content: &'a dyn IContent<'a, T>,
	view_item: impl Fn(usize, &'a T) -> Element<'a, Message, Theme, Renderer> + 'a,
) -> SparseList<'a, T, Message, Theme, Renderer> {
	SparseList::new(content, view_item)
}

/// This *does* implement the `iced_code::widget::operation::Scrollable` interface but in a slightly
/// nonstandard way.
///
/// - `snap_to` only responds to relative offsets of 0.0 (top) and 1.0 (bottom). any other value
///   does nothing.
/// - `scroll_to` misuses the `AbsoluteOffset` fields. both must be `Some` or else it does nothing.
///   `y` must be the jumped-to item index *converted to an `f32` using `f32::from_bits()`.* `x` is
///   a normal float, but it is instead interpreted as the desired Y position of the item measured
///   from the top of the list's view. e.g. `0.0` puts the item at the top, `100.0` puts it 100
///   pixels below the top, and `-50.0` puts it 50 pixels *above* the top.
/// - `scroll_by` works normally, though the `x` component is ignored.
#[allow(missing_debug_implementations)]
pub struct SparseList<'a, T, Message, Theme, Renderer> {
	id: Option<widget::Id>,
	content: &'a dyn IContent<'a, T>,
	view_item:
		Box<dyn Fn(usize, &'a T) -> Element<'a, Message, Theme, Renderer> + 'a>,
	visible_elements: BTreeMap<usize, Element<'a, Message, Theme, Renderer>>,
}

impl<'a, T, Message, Theme, Renderer: iced_core::Renderer>
	SparseList<'a, T, Message, Theme, Renderer> {
	pub fn new(
		content: &'a dyn IContent<'a, T>,
		view_item: impl Fn(usize, &'a T) -> Element<'a, Message, Theme, Renderer>
			+ 'a,
	) -> Self {
		println!("--------------------------NEW LIST-----------------------------");
		Self {
			id: None,
			content,
			view_item: Box::new(view_item),
			visible_elements: BTreeMap::new(),
		}
	}

	/// Sets the [`widget::Id`] of the [`Scrollable`].
	pub fn id(mut self, id: impl Into<widget::Id>) -> Self {
		self.id = Some(id.into());
		self
	}

	// fn new_element(&self, idx: usize) -> Element<'a, Message, Theme, Renderer> {
	// 	self.new_element_with(idx, &self.content.get(idx).unwrap())
	// }

	fn new_element_with(&self, idx: usize, item: &'a T) -> Element<'a, Message, Theme, Renderer> {
		(self.view_item)(idx, item)
	}

	// passing the element in and returning it seems silly but BORROW CHECKER REASONS (well,
	// really, "lack of impl Borrow for &mut Element" reasons but same diff)
	fn layout_element(&self, mut tree: &mut Tree, limits: &layout::Limits, renderer: &Renderer,
		y: f32, mut element: Element<'a, Message, Theme, Renderer>)
	-> (Element<'a, Message, Theme, Renderer>, layout::Node) {
		let layout = element
			.as_widget_mut()
			.layout(&mut tree, renderer, &limits)
			.move_to((0.0, y));

		(element, layout)
	}

	fn elements_need_to_be_recreated(&self, state: &State) -> bool {
		// println!("ELEMENTS = {}, LAYOUTS = {}",
		// 	self.visible_elements.len(), state.visible_layouts.len());
		assert!((self.visible_elements.len() == state.visible_layouts.len()) ||
			(self.visible_elements.is_empty() && !state.visible_layouts.is_empty()));

		self.visible_elements.is_empty() && !state.visible_layouts.is_empty()
	}

	/// recreate elements from layouts after this list has been recreated.
	fn recreate_elements(&mut self, state: &State) {
		for (idx, _) in state.visible_layouts.iter() {
			// if the item was removed from self.content, don't recreate the element.
			if let Some(item) = self.content.get(*idx) {
				self.visible_elements.insert(*idx, self.new_element_with(*idx, item));
			}
		}

		self.dump_visible_indexes();
	}

	/// recomputes all elements' y positions so they start at 0.0 and increase.
	fn recompute_element_y_positions(&self, state: &mut State) {
		let mut current_y = 0.0;

		for (_, (layout, _)) in state.visible_layouts.iter_mut() {
			layout.move_to_mut((0.0, current_y));
			current_y += layout.size().height;
		}
	}

	/// recomputes elements' y positions after the given index.
	fn recompute_element_y_positions_after(&self, state: &mut State, idx: usize,
	mut current_y: f32) {
		for (_, (layout, _)) in state.visible_layouts
			.range_mut((Bound::Excluded(idx), Bound::Unbounded))
		{
			layout.move_to_mut((0.0, current_y));
			current_y += layout.size().height;
		}
	}


	/// called when the width of the container changes.
	fn relayout_items(&mut self, state: &mut State, renderer: &Renderer) {
		let limits = state.limits_without_max_height();

		for ((_, element), (_, (layout, tree))) in
			self.visible_elements.iter_mut().zip(&mut state.visible_layouts)
		{
			let y = layout.bounds().y;
			*layout = element
				.as_widget_mut()
				.layout(tree, renderer, &limits)
				.move_to((0.0, y));
		}
	}

	/// add an element to the list. `y` is the Y position it should be placed at; `heightfn` is a
	/// callback called with the height of the newly-created item, useful for updating state.
	fn add_element(&mut self, state: &mut State, renderer: &Renderer, idx: usize, item: &'a T,
	y: f32, mut heightfn: impl FnMut(f32) -> ()) {
		let element = self.new_element_with(idx, item);
		let mut tree = Tree::new(&element);
		let (element, layout) = self.layout_element(&mut tree, &state.limits_without_max_height(),
			renderer, y, element);

		let height = layout.size().height;
		state.content_bounds.height += height;
		heightfn(height);

		state.visible_layouts.insert(idx, (layout, tree));
		self.visible_elements.insert(idx, element);
	}

	/// remove an element from the list. `heightfn` is a callback called with the height of the
	/// item about to be removed.
	fn remove_element(&mut self, state: &mut State, idx: usize,
	mut heightfn: impl FnMut(f32) -> ()) {
		let height = state.height_of(idx);
		// I'm paranoid about float arithmetic okay
		state.content_bounds.height = (state.content_bounds.height - height).max(0.0);
		heightfn(height);

		state.visible_layouts.remove(&idx);
		self.visible_elements.remove(&idx);

		// I hate floats.
		if state.visible_layouts.is_empty() {
			state.content_bounds.height = 0.0;
		}
	}

	/// returns the required scroll offset to put the item at `original_idx` at the right Y position
	/// in the list's view (or as close to that as possible, in the case that there aren't enough
	/// items before it).
	fn add_elements_before(&mut self, state: &mut State, renderer: &Renderer, original_idx: usize,
	mut pixels_remaining: f32) -> f32 {
		// println!("+ adding elements before {} ({} pixels)", original_idx, pixels_remaining);
		for (idx, item) in self.content.items_before(original_idx) {
			if pixels_remaining <= 0.0 { break; }
			self.add_element(state, renderer, idx, item, 0.0, |height| {
				pixels_remaining -= height;
			});
		}

		// now recompute their Y positions so that they start at Y = 0.
		self.recompute_element_y_positions(state);
		pixels_remaining
	}

	/// returns the number of pixels remaining after adding the elements, which may be nonzero
	/// if we ran out of elements (at the end of the list).
	fn add_elements_after(&mut self, state: &mut State, renderer: &Renderer, original_idx: usize,
	mut current_y: f32, mut pixels_remaining: f32) -> f32 {
		// println!("+ adding elements after {} ({} pixels)", original_idx, pixels_remaining);
		let mut ran_out_of_items = true;
		for (idx, item) in self.content.items_after(original_idx) {
			if pixels_remaining <= 0.0 {
				ran_out_of_items = false;
				break;
			}
			self.add_element(state, renderer, idx, item, current_y, |height| {
				current_y += height;
				pixels_remaining -= height;
			});
		}

		// PARANOID ABOUT FLOAT ARITHMETIC...
		if ran_out_of_items { pixels_remaining } else { 0.0 }
	}

	/// removes elements at start of view as long as their bottom is below `view_top`. returns the
	/// new updated scroll offset.
	fn remove_elements_above(&mut self, state: &mut State, view_top: f32) -> f32 {
		// println!("- removing elements above {}", view_top);
		let mut pixels_removed = 0.0;

		while let Some((idx, bounds)) =
			state.visible_layouts.first_key_value().map(|(k, v)| (*k, v.0.bounds()))
		{
			if bounds.bottom() < view_top {
				self.remove_element(state, idx, |height| { pixels_removed += height; });
			} else {
				break;
			}
		}

		self.recompute_element_y_positions(state);
		view_top - pixels_removed
	}

	/// removes elements at end of view as long as their top is below `view_bottom`.
	fn remove_elements_below(&mut self, state: &mut State, view_bottom: f32) {
		// println!("- removing elements below {}", view_bottom);
		while let Some((idx, bounds)) =
			state.visible_layouts.last_key_value().map(|(k, v)| (*k, v.0.bounds()))
		{
			if bounds.y > view_bottom {
				self.remove_element(state, idx, |_|{});
			} else {
				break;
			}
		}
	}

	fn apply_changes(&mut self, state: &mut State, renderer: &Renderer) {
		let mut changes = self.content.changes();

		while let Some(change) = changes.pop_front() {
			match change {
				Change::Changed { idx } => self.item_changed(state, renderer, idx),
				Change::Removed { idx } => self.item_removed(state, idx),
				Change::Added   { idx } => self.item_added  (state, renderer, idx),
			}
		}
	}

	fn item_changed(&mut self, state: &mut State, renderer: &Renderer, idx: usize) {
		if state.visible_layouts.contains_key(&idx) {
			assert!(self.visible_elements.contains_key(&idx));

			// recreate and re-lay-out, move other layouts up/down
			let mut height_delta: f32 = 0.0;
			let y = state.y_of(idx);
			self.remove_element(state, idx, |height| { height_delta -= height; });
			self.add_element(state, renderer, idx,
				self.content.get(idx).expect("item_changed on an item not in the content"),
				y, |height| { height_delta += height; });

			if height_delta != 0.0 {
				self.recompute_element_y_positions_after(
					state, idx, state.bottom_of(idx));

				// we won't deal with spawning/removing items here - just mark self as dirty and
				// let that happen during the draw call
				state.changes_happened = true;
			}
		}
	}

	fn item_removed(&mut self, state: &mut State, idx: usize) {
		if state.visible_layouts.contains_key(&idx) {
			let old_y = state.y_of(idx);
			// it's possible self.visible_elements doesn't contain idx, if the list was recreated
			// and that content was deleted.
			self.remove_element(state, idx, |_| {});
			self.recompute_element_y_positions_after(state, idx, old_y);
			state.changes_happened = true;
		}
	}

	fn item_added(&mut self, state: &mut State, renderer: &Renderer, idx: usize) {
		if state.visible_layouts.is_empty() {
			// add it!
			self.add_element(state, renderer, idx,
				self.content.get(idx).expect("item_added on an item not in the content"),
				0.0, |_|{});
			state.changes_happened = true;
		} else {
			if self.is_between_visible(idx, state) {
				// figure out which element it's after to know Y
				let y = self.y_pos_of_new_item(idx, state);
				// add it
				self.add_element(state, renderer, idx,
					self.content.get(idx).expect("item_added on an item not in the content"),
					y, |_| {});
				// shift everything after it down
				self.recompute_element_y_positions_after(
					state, idx, state.bottom_of(idx));
			} else if self.is_right_after_visible(idx, state) {
				// this case is possible when there aren't many items in the list, and the
				// newly-added item is right after the last item.

				// don't have to add it here, it'll get added in update()
				state.changes_happened = true;
			}
		}
	}

	fn y_pos_of_new_item(&self, new_idx: usize, state: &State) -> f32 {
		let mut iter = state.visible_layouts.iter()
			.skip_while(|(idx, _)| **idx < new_idx);
		state.y_of(*iter.next()
			.expect("y_pos_of_new_item called with index not between existing indices").0)
	}

	fn is_between_visible(&self, idx: usize, state: &State) -> bool {
		idx > state.first_index() && idx < state.last_index()
	}

	fn is_right_after_visible(&self, idx: usize, state: &State) -> bool {
		match self.content.items_after(state.last_index()).next() {
			Some((idx_after, _)) if idx == idx_after => return true,
			_ => {}
		}

		false
	}

	fn refresh(&mut self, state: &mut State, renderer: &Renderer, bounds: Rectangle) -> Vector {
		assert!(!self.content.is_empty(), "refresh called with no items in content");

		// there are four possible cases:
		//
		// - there are enough elements above and below to fill up the whole view
		// - there are enough elements above, but *not* enough below (towards end of list)
		//     - in this case, the desired offset_y is not achievable - must move it **down**
		// - there are enough elements below, but *not* enough above (towards beginning of list)
		//     - in this case, the desired offset_y is not achievable - must move it **up**
		// - there are not enough elements above or below (short list)
		//     - in this case, the desired offset_y is not achievable - **it's set to 0**
		//
		// ofc we have to do this sequentially so it's a little weird

		// ------------------------------------------------------------
		// setup

		// wipe out old state
		self.visible_elements.clear();
		state.visible_layouts.clear();
		state.content_bounds = Rectangle { x: 0.0, y: 0.0, width: bounds.width, height: 0.0 };

		// ------------------------------------------------------------
		// initial element (the one they asked to jump to)

		// grab the new position and offset. offset_y is where (measured from the top of the view)
		// the new element should be positioned.

		let (initial_idx, mut offset_y) = match std::mem::take(&mut state.new_position)
			.expect("refresh without new_position")
		{
			// SAFETY: both unwrap()s okay because we asserted content is not empty at top
			NewPosition::Top                        => (self.content.first().unwrap(), 0.0),
			NewPosition::Bottom                     => (self.content.last().unwrap(), 0.0),
			NewPosition::Absolute { idx, offset_y } => (idx, offset_y),
		};

		// TODO: is there any requirement on offset_y? I could see it being useful to e.g. position
		// the top of the element off the top of the view, but there's still some kind of sane
		// limit... that wouldn't really be checked here, but rather on the method which set
		// state.new_position in the first place. I don't think we'd assert!() it, just clamp
		// it to some sane range (e.g. no higher than "would put bottom of initial_element above
		// top of screen" and no lower than bottom of screen)

		// we need to first create the element from the item at initial_idx. its Y position doesn't
		// matter because it gets recomputed by add_elements_before later.
		self.add_element(state, renderer, initial_idx,
			&self.content.get(initial_idx).expect("refresh on an item not in the content"),
			0.0, |_|{});

		// ------------------------------------------------------------
		// if needed, we need to create elements before it...
		let top_pixels_left = offset_y;
		let top_pixels_left = self.add_elements_before(
			state, renderer, initial_idx, top_pixels_left);

		if top_pixels_left > 0.0 {
			// ran out of elements on top; that means offset_y is not achievable.
			// have to slide it up.
			offset_y -= top_pixels_left;
			assert_eq!(state.y_of(initial_idx), offset_y);
		}

		// ------------------------------------------------------------
		// then we might need to create elements *after* it...
		let initial_bounds = state.bounds_of(initial_idx);

		let bottom_pixels_left = bounds.height - (offset_y + initial_bounds.height);
		let bottom_pixels_left = self.add_elements_after(
				state, renderer, initial_idx, initial_bounds.bottom(), bottom_pixels_left);

		// note that we may have run out of items to fill up the view, but that's okay. in that case
		// it just won't be scrollable.

		if bottom_pixels_left > 0.0 {
			// ran out of elements at the bottom; that means offset_y is not achievable.

			// SAFETY: asserted content is not empty at start of function
			if state.first_index() == self.content.first().unwrap() {
				// in this case, we're just out of elements!
				// the desired scroll offset is forced to 0.
				state.offset_y = 0.0;
				assert_eq!(state.y_of(state.first_index()), 0.0);
				return Vector::ZERO;
			} else {
				// have to slide it down.
				offset_y += bottom_pixels_left;

				// buuuuuut since we slid down... it may be the case that we need to do another
				// round of adding elements before!!! and THAT can run out too... sheesh
				let top_pixels_left = offset_y;
				let top_pixels_left = self.add_elements_before(
					state, renderer, state.first_index(), top_pixels_left);

				if top_pixels_left > 0.0 {
					state.offset_y = 0.0;
					assert_eq!(state.y_of(state.first_index()), 0.0);
					return Vector::ZERO;
				}
			}
		}

		// for scrolling, reset the state's scrolling offset and return the desired delta.
		state.offset_y = 0.0;
		assert_eq!(state.y_of(state.first_index()), 0.0);
		Vector::new(0.0, state.y_of(initial_idx) - offset_y)
	}

	/// about to scroll by `delta`; check if we need to manifest new items in the direction of
	/// scrolling (and delete old ones that fall off the other end). returns the adjusted delta
	/// to be passed to state.scroll()
	///
	/// also this name is not the best, since this is sometimes called with a delta of 0 to just
	/// spawn/remove items when e.g. the bounds changed or items were added/removed/changed
	fn try_scroll(&mut self, state: &mut State, renderer: &Renderer, bounds: Rectangle,
	mut delta: Vector) -> Vector {
		// nothing to do if there are no items to display.
		if self.content.is_empty() || state.visible_layouts.is_empty() {
			return delta;
		}

		// compute current and desired scroll offsets.
		let cur_offset_y = state.absolute_offset(bounds.height, state.content_bounds.height);
		let new_offset_y = cur_offset_y + delta.y;

		let mut new_view_top = new_offset_y;
		let mut new_view_bottom = new_view_top + bounds.height;

		// add elements after the last visible element if it moves up past the bottom.
		let last_bottom = state.last_bottom();
		let mut bottom_pixels_needed = new_view_bottom - last_bottom;

		if bottom_pixels_needed > 0.0 {
			bottom_pixels_needed = self.add_elements_after(
				state, renderer, state.last_index(), last_bottom, bottom_pixels_needed);
		}

		// remove elements at the top as they move offscreen.
		// bottom_pixels_needed == 0.0 is fine here because add_elements_after returns exactly 0.0 if
		// we ran out of items
		if bottom_pixels_needed == 0.0 && state.first_bottom() < new_offset_y {
			let new_offset_y = self.remove_elements_above(state, new_offset_y);
			state.offset_y = 0.0;
			delta = Vector::new(0.0, new_offset_y);
		}

		// if top-most element's top moves below new_offset_y, need more items at top.
		let top_pixels_needed = state.first_top() - new_view_top;

		if top_pixels_needed > 0.0 {
			let top_pixels_needed = self.add_elements_before(
				state, renderer, state.first_index(), top_pixels_needed);

			// if top_pixels_needed > 0, we ran out of items, so the scroll offset will be 0.
			// if top_pixels_needed is negative, there are items that will be partially scrolled-off
			// the top of the view, and the scroll offset needs to be the absolute value of
			// that, so that they are moved the appropriate distance up.
			let new_offset_y = top_pixels_needed.min(0.0).abs();
			state.offset_y = 0.0;
			delta = Vector::new(0.0, new_offset_y);

			new_view_top = new_offset_y;
			new_view_bottom = new_view_top + bounds.height;
		}

		// remove elements at the bottom as they move offsecreen
		if state.last_top() > new_view_bottom {
			self.remove_elements_below(state, new_view_bottom);
		}

		self.dump_visible_indexes();
		delta
	}

	fn dump_visible_indexes(&self) {
		print!("visible: [");

		let mut first = true;

		for (idx, _) in self.visible_elements.iter() {
			if first {
				first = false;
			} else {
				print!(", ");
			}
			print!("{:04X}", idx);
		}

		println!("]");
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
		let new_position = match self.content.first() {
			Some(idx) => Some(NewPosition::Absolute { idx, offset_y: 0.0 }),
			None      => Some(NewPosition::Top),
		};
			// TODO: temporary
			// self.content.last().map(|idx| NewPosition { idx, offset_y: 20.0 });
			// self.content.items_after(self.content.first().unwrap())
			// .nth(9).map(|(idx, _)| NewPosition { idx, offset_y: 10.0 });

		tree::State::new(State {
			last_limits:        layout::Limits::NONE,
			last_bounds:        Rectangle::default(),
			changes_happened:   false,
			visible_layouts:    BTreeMap::new(),
			content_bounds:     Rectangle::default(),
			new_position,
			offset_y:           0.0,
			keyboard_modifiers: keyboard::Modifiers::default(),
			last_scrolled:      None,
			scroll_by:          None,
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
		let mut state = tree.state.downcast_mut::<State>();
		state.last_limits = limits.loose();

		if !state.needs_refresh() && !self.elements_need_to_be_recreated(state) {
			self.apply_changes(&mut state, renderer);
		}

		layout::Node::new(limits.resolve(Length::Fill, Length::Fill, state.content_bounds.size()))
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
		let mut state = tree.state.downcast_mut::<State>();

		let bounds = layout.bounds();

		if bounds != state.last_bounds {
			// if the width changed, change the widths of all the visible items.
			if bounds.width != state.last_bounds.width {
				self.relayout_items(state, renderer);
			}

			// if the height changed, that could cause items to need to be added/removed.
			if bounds.height != state.last_bounds.height {
				self.try_scroll(state, renderer, bounds, Vector::ZERO);
			}

			state.last_bounds = bounds;
		}

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
				state.last_scrolled = None;
			}
		}

		let offset = layout.position() - Point::ORIGIN;
		let cursor_over_scrollable = cursor.position_over(bounds);

		if state.last_scrolled.is_none()
			|| !matches!(event, Event::Mouse(mouse::Event::WheelScrolled { .. }))
		{
			let translation = state.translation(bounds, state.content_bounds);

			let cursor = match cursor_over_scrollable {
				Some(cursor_position) => mouse::Cursor::Available(cursor_position + translation),
				_                     => cursor.levitate() + translation,
			};

			let had_input_method = shell.input_method().is_enabled();

			for ((_, element), (_, (layout, tree))) in
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
			return;
		}

		// if the event was already captured, return.
		if shell.is_event_captured() {
			return;
		}

		match event {
			Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
				if cursor_over_scrollable.is_none() {
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
				state.scroll_and_capture(delta, bounds, state.content_bounds, shell);
			}
			Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
				// check for keyboard modifiers (to undo shift-scroll axis-swapping on macos)
				state.keyboard_modifiers = *modifiers;
			}
			Event::Window(window::Event::RedrawRequested(_)) => {
				if state.needs_refresh() {
					let delta = self.refresh(state, renderer, bounds);
					state.scroll_and_capture(delta, bounds, state.content_bounds, shell);
				} else {
					if self.elements_need_to_be_recreated(state) {
						// this SparseList was recreated - the elements must be recreated from the
						// layouts in the state.
						self.recreate_elements(state);

						// and apply any pending changes, now that everything is recreated.
						self.apply_changes(&mut state, renderer);
					}

					if state.changes_happened {
						self.try_scroll(state, renderer, bounds, Vector::ZERO);
						state.changes_happened = false;
					}

					if let Some(delta) = state.scroll_by {
						println!("REDRAW: scrolling by {}", delta);
						let delta = self.try_scroll(state, renderer, bounds,
							Vector::new(0.0, delta));
						state.scroll_and_capture(delta, bounds, state.content_bounds, shell);
						state.scroll_by = None;
					}
				}
			}

			_ => {}
		}

		if last_offset_y != state.offset_y {
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

		let Some(visible_bounds) = bounds.intersection(viewport) else {
			return;
		};

		let translation = state.translation(bounds, state.content_bounds);

		let cursor = match cursor.position_over(bounds) {
			Some(cursor_position) => mouse::Cursor::Available(cursor_position + translation),
			_                     => cursor.levitate() + translation,
		};

		let offset = layout.position() - Point::ORIGIN;

		renderer.with_layer(visible_bounds, |renderer| {
			renderer.with_translation(
				Vector::new(-translation.x, -translation.y),
				|renderer| {
					for ((_, element), (_, (layout, tree))) in
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
			.map(|((_, element), (_, (layout, tree)))| {
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

		let bounds = layout.bounds();
		let translation = state.translation(bounds, state.content_bounds);

		operation.scrollable(self.id.as_ref(), bounds, state.content_bounds, translation, state);

		let offset = layout.position() - Point::ORIGIN;

		for ((_, element), (_, (layout, tree))) in
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
			.filter_map(|((_, child), (_, (layout, tree)))| {
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

// ------------------------------------------------------------------------------------------------
// State
// ------------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum NewPosition {
	Top,
	Bottom,
	Absolute { idx: usize, offset_y: f32 },
}

struct State {
	last_limits:        layout::Limits,
	last_bounds:        Rectangle,
	changes_happened:   bool,
	visible_layouts:    BTreeMap<usize, (layout::Node, Tree)>,
	content_bounds:     Rectangle,

	// scrolling stuff
	offset_y:           f32,
	keyboard_modifiers: keyboard::Modifiers,
	last_scrolled:      Option<Instant>,
	new_position:       Option<NewPosition>,
	scroll_by:          Option<f32>
}

impl operation::Scrollable for State {
	fn snap_to(&mut self, offset: RelativeOffset<Option<f32>>) {
		State::snap_to(self, offset);
	}

	fn scroll_to(&mut self, offset: AbsoluteOffset<Option<f32>>) {
		State::scroll_to(self, offset);
	}

	fn scroll_by(&mut self, offset: AbsoluteOffset, _bounds: Rectangle,
	_content_bounds: Rectangle) {
		State::scroll_by(self, offset.y);
	}
}

impl State {
	fn needs_refresh(&self) -> bool {
		self.new_position.is_some()
	}

	fn limits_without_max_height(&self) -> layout::Limits {
		layout::Limits::new(
			self.last_limits.min(),
			Size::new(self.last_limits.max().width, f32::INFINITY)
		)
	}

	fn bounds_of(&self, idx: usize) -> Rectangle {
		self.visible_layouts.get(&idx).expect("bounds_of on a nonexistent item").0.bounds()
	}

	fn y_of(&self, idx: usize) -> f32 {
		self.bounds_of(idx).y
	}

	fn height_of(&self, idx: usize) -> f32 {
		self.bounds_of(idx).height
	}

	fn bottom_of(&self, idx: usize) -> f32 {
		self.bounds_of(idx).bottom()
	}

	fn first_index(&self) -> usize {
		*self.visible_layouts.first_key_value().expect("first_index with no items in content").0
	}

	fn last_index(&self) -> usize {
		*self.visible_layouts.last_key_value().expect("last_index with no items in content").0
	}

	fn first_layout(&self) -> &layout::Node {
		&self.visible_layouts.first_key_value().expect("first_layout with no items in content").1.0
	}

	fn last_layout(&self) -> &layout::Node {
		&self.visible_layouts.last_key_value().expect("last_layout with no items in content").1.0
	}

	fn first_top(&self) -> f32 {
		self.first_layout().bounds().y
	}

	fn last_top(&self) -> f32 {
		self.last_layout().bounds().y
	}

	fn first_bottom(&self) -> f32 {
		self.first_layout().bounds().bottom()
	}

	fn last_bottom(&self) -> f32 {
		self.last_layout().bounds().bottom()
	}

	// Scrolling stuff
	fn scroll(&mut self, delta: Vector<f32>, bounds: Rectangle, content_bounds: Rectangle) {
		if bounds.height < content_bounds.height {
			self.offset_y = (self.absolute_offset(bounds.height, content_bounds.height) + delta.y)
				.clamp(0.0, content_bounds.height - bounds.height);

			// println!(":::::::::: scroll - State::scroll(), offset_y = {:?}", self.offset_y);
		} else {
			// println!(":::::::::: scroll - State::scroll() didn't scroll, bounds = {:?}, \
			// 	content_bounds = {:?}", bounds, content_bounds);
		}
	}

	fn scroll_and_capture<Message>(&mut self, delta: Vector<f32>, bounds: Rectangle,
	content_bounds: Rectangle, shell: &mut Shell<'_, Message>) {
		self.scroll(delta, bounds, content_bounds);

		if content_bounds.width > bounds.width || content_bounds.height > bounds.height {
			self.last_scrolled = Some(Instant::now());
			shell.capture_event();
		}
	}

	fn snap_to(&mut self, offset: RelativeOffset<Option<f32>>) {
		if let Some(y) = offset.y {
			let y = y.clamp(0.0, 1.0);

			if y == 0.0 {
				println!(":::::::::: scroll - State::snap_to(Top)");
				self.new_position = Some(NewPosition::Top);
			} else if y == 1.0 {
				println!(":::::::::: scroll - State::snap_to(Bottom)");
				self.new_position = Some(NewPosition::Bottom);
			}
		}
	}

	fn scroll_to(&mut self, offset: AbsoluteOffset<Option<f32>>) {
		if let (Some(offset_y), Some(idx)) = (offset.x, offset.y) {
			let idx = idx.to_bits() as usize;
			println!(":::::::::: scroll - State::scroll_to({:04X}) {}", idx, offset_y);
			self.new_position = Some(NewPosition::Absolute { idx, offset_y });
		}
	}

	fn scroll_by(&mut self, offset: f32) {
		println!(":::::::::: scroll - State::scroll_by({})", offset);
		self.scroll_by = Some(offset);
	}

	fn absolute_offset(&self, viewport: f32, content: f32) -> f32 {
		self.offset_y.min((content - viewport).max(0.0))
	}

	/// Returns the scrolling translation of the [`State`], given
	/// the bounds of the [`Scrollable`] and its contents.
	fn translation(&self, bounds: Rectangle, content_bounds: Rectangle) -> Vector {
		Vector::new(
			0.0,
			self.absolute_offset(bounds.height, content_bounds.height).round()
		)
	}
}