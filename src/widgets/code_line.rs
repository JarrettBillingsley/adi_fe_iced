
use iced_core::{
	Renderer, Widget, Layout, Rectangle, Point, Vector, Shell, Clipboard, overlay,
	widget::{ self, Tree, tree::{ Tag, State as TreeState } },
	mouse::{ self, Cursor, Button as MouseButton, Click, Event as MouseEvent,
		click::Kind as ClickKind },
	keyboard::{ Event as KeyboardEvent, Modifiers as KeyModifiers, Key, key::Named as NamedKey },
	layout::{ Limits, Node },
	renderer::{ self, Style },
};

use iced::{
	Element, Color as IcedColor, color, Size, Length, Theme, Event,
	widget::{ text, },
};

use adi::{ EA, PrintStyle };

use crate::{ CONSOLAS_FONT_BOLD };
use crate::ui::*;
use crate::widgets::code_view::{ OperandLocation, CodeViewMessage };

// ------------------------------------------------------------------------------------------------
// PrintStyleEx
// ------------------------------------------------------------------------------------------------

/// Extended printing style enumeration for more things than ADI provides.
#[non_exhaustive]
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
enum PrintStyleEx {
	Plain,
	SegName,
	CodeBytes,
	Unknown,
	Error,
	Adi(PrintStyle),
}

impl From<PrintStyle> for PrintStyleEx {
	fn from(other: PrintStyle) -> Self {
		Self::Adi(other)
	}
}

impl From<Option<PrintStyle>> for PrintStyleEx {
	fn from(other: Option<PrintStyle>) -> Self {
		use PrintStyle::*;
		match other {
			None             => Self::Plain,
			Some(Operand(_)) => panic!("trying to turn operand into a style"),
			Some(ps)         => Self::Adi(ps),
		}
	}
}

// ------------------------------------------------------------------------------------------------
// color_of
// ------------------------------------------------------------------------------------------------

fn color_of(style: impl Into<PrintStyleEx>) -> IcedColor {
	use PrintStyle::*;
	use PrintStyleEx::*;
	match style.into() {
		// TODO: make colors configurable
		Plain           => color!(0xFFFFFF), // white
		SegName         => color!(0xFFFF00), // yellow
		CodeBytes       => color!(0x8080FF), // light blue
		Unknown         => color!(0xFF7F00), // orange
		Error           => color!(0xFF4040), // light red
		Adi(Mnemonic)   => color!(0xFF0000), // red
		Adi(Register)   => color!(0xFFFFFF), // white
		Adi(Number)     => color!(0x00FF00), // bright green
		Adi(Symbol)     => color!(0xFFFFFF), // white
		Adi(String)     => color!(0xFF7F00), // orange
		Adi(Comment)    => color!(0x00AF00), // dark green
		Adi(Refname)    => color!(0xFFFFB0), // light tan
		Adi(Label)      => color!(0xA06000), // light brown
		Adi(Operand(_)) => panic!("trying to get the color of an operand"),
		Adi(_)          => panic!("a new PrintStyle was added!"),
	}
}

// ------------------------------------------------------------------------------------------------
// Building children
// ------------------------------------------------------------------------------------------------

struct LinePiece {
	text:  String,
	style: PrintStyleEx,
	opn:   Option<u8>,
}

impl LinePiece {
	fn new(text: impl Into<String>, style: impl Into<PrintStyleEx>) -> Self {
		Self {
			text: text.into(),
			style: style.into(),
			opn: None,
		}
	}

	fn new_op(text: impl Into<String>, style: impl Into<PrintStyleEx>, opn: u8) -> Self {
		Self {
			text: text.into(),
			style: style.into(),
			opn: Some(opn),
		}
	}
}

#[allow(unused)]
struct LineSpan {
	/// 0-based character index of where this span starts, measured from left side of line
	start: usize,

	/// how many characters are in this span
	len:   usize,

	/// if this is an operand, Some(operand_idx)
	opn:   Option<u8>,
}

struct LineChildren<'a> {
	total_len: usize,
	children:  Vec<Element<'a, CodeViewMessage>>,
	spans:     Vec<LineSpan>,
}

fn codetext(s: impl Into<String>, style: impl Into<PrintStyleEx>)
-> Element<'static, CodeViewMessage> {
	text(s.into())
		.font(CONSOLAS_FONT_BOLD) // TODO: make font customizable
		.color(color_of(style.into()))
		.into()
}

fn build_children<'a>(pieces: Vec<LinePiece>) -> LineChildren<'a> {
	let mut children  = Vec::with_capacity(pieces.len());
	let mut spans     = Vec::with_capacity(pieces.len());
	let mut start     = 0;

	for LinePiece { text, style, opn } in pieces.into_iter() {
		spans.push(LineSpan { start, len: text.len(), opn });
		start += text.len();
		children.push(codetext(text, style));
	}

	LineChildren { total_len: start, children, spans }
}

// ------------------------------------------------------------------------------------------------
// LineKind
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct ChildIdx(usize);

#[allow(unused)]
enum LineKind {
	Blank   { dummy:   ChildIdx },
	Error   { message: ChildIdx },
	Comment { comment: ChildIdx },
	Label   { label:   ChildIdx },
	Code {
		bb_ea:      EA,
		instn:      usize,
		code_bytes: ChildIdx,
		mnemonic:   ChildIdx,
		operands:   Vec<(CodeOpData, ChildIdx)>
		// TODO: outrefs: ChildIdx,
	},
	Unknown { bytes: ChildIdx },
	// TODO: data
}

// ------------------------------------------------------------------------------------------------
// CodeLine
// ------------------------------------------------------------------------------------------------

#[allow(unused)]
pub(crate) struct CodeLine<'a> {
	width:     Length,
	height:    Length,
	children:  Vec<Element<'a, CodeViewMessage>>,
	spans:     Vec<LineSpan>,
	total_len: usize,

	ea:        EA,
	text_ea:   Option<(ChildIdx, ChildIdx)>,
	kind:      LineKind,
}

impl<'a> CodeLine<'a> {
	// --------------------------------------------------------------------------------------------
	// Constructors

	fn new(ea: EA, text_ea: Option<(ChildIdx, ChildIdx)>, children: LineChildren<'a>,
	kind: LineKind) -> Self {
		assert!(!children.children.is_empty());
		Self {
			width:     Length::Shrink,
			height:    Length::Shrink,
			children:  children.children,
			spans:     children.spans,
			total_len: children.total_len,
			ea,
			text_ea,
			kind,
		}.adjust_size()
	}

	pub(crate) fn new_blank(ea: EA) -> Self {
		Self::new(ea, None,
			build_children(vec![
				LinePiece::new("", PrintStyleEx::Plain), // 0
			]),
			LineKind::Blank {
				dummy: ChildIdx(0),
			})
	}

	pub(crate) fn new_error(ea: EA, text_ea: TextEA, message: String) -> Self {
		Self::new(ea, Some((ChildIdx(0), ChildIdx(1))),
			build_children(vec![
				LinePiece::new(text_ea.seg,                   PrintStyleEx::SegName), // 0
				LinePiece::new(format!(":{} ", text_ea.offs), PrintStyleEx::Plain),   // 1
				LinePiece::new(message,                       PrintStyleEx::Error),   // 2
			]),
			LineKind::Error {
				message: ChildIdx(2),
			})
	}

	pub(crate) fn new_comment(ea: EA, comment: String) -> Self {
		Self::new(ea, None,
			build_children(vec![
				LinePiece::new(format!("; {}", comment), PrintStyle::Comment), // 0
			]),
			LineKind::Comment {
				comment: ChildIdx(0),
			})
	}

	pub(crate) fn new_label(ea: EA, label: String) -> Self {
		assert!(!label.is_empty());
		Self::new(ea, None,
			build_children(vec![
				LinePiece::new(label, PrintStyle::Label),   // 0
				LinePiece::new(":",   PrintStyleEx::Plain), // 1
			]),
			LineKind::Label {
				label: ChildIdx(0),
			})
	}

	pub(crate) fn new_code(ea: EA, text_ea: TextEA, bb_ea: EA, instn: usize, code_bytes: String,
	mnemonic: String, operands: Vec<CodeOpData>) -> Self {
		let code_bytes = format!("{:8}     ", code_bytes);
		let mut children = vec![
			LinePiece::new(text_ea.seg,                   PrintStyleEx::SegName),   // 0
			LinePiece::new(format!(":{} ", text_ea.offs), PrintStyleEx::Plain),     // 1
			LinePiece::new(code_bytes,                    PrintStyleEx::CodeBytes), // 2
			LinePiece::new(mnemonic,                      PrintStyle::Mnemonic),    // 3
		];

		let child_first_idx = children.len();

		children.extend(operands.iter().map(|op|                                    // 4, 5, ...
			match op.opn {
				Some(opn) => LinePiece::new_op(op.text.clone(), op.style, opn),
				None      => LinePiece::new   (op.text.clone(), op.style),
			}));

		Self::new(ea, Some((ChildIdx(0), ChildIdx(1))),
			build_children(children),
			LineKind::Code {
				bb_ea,
				instn,
				code_bytes: ChildIdx(2),
				mnemonic:   ChildIdx(3),
				operands: operands.into_iter().enumerate()
					.map(|(i, op)| (op, ChildIdx(child_first_idx + i))).collect()
			})
	}

	pub(crate) fn new_unknown(ea: EA, text_ea: TextEA, bytes: String) -> Self {
		Self::new(ea, Some((ChildIdx(0), ChildIdx(1))),
			build_children(vec![
				LinePiece::new(text_ea.seg,                   PrintStyleEx::SegName), // 0
				LinePiece::new(format!(":{} ", text_ea.offs), PrintStyleEx::Plain),   // 1
				LinePiece::new(bytes,                         PrintStyleEx::Unknown), // 2
			]),
			LineKind::Unknown {
				bytes: ChildIdx(2),
			})
	}

	// --------------------------------------------------------------------------------------------
	// Layout stuff

	fn adjust_size(mut self) -> Self {
		let (mut width, mut height) = (self.width, self.height);

		for child in self.children.iter() {
			let child_size = child.as_widget().size_hint();

			if !child_size.is_void() {
				width = width.enclose(child_size.width);
				height = height.enclose(child_size.height);
			}
		}

		(self.width, self.height) = (width, height);
		self
	}

	fn layout_children(&mut self, tree: &mut Tree, renderer: &iced::Renderer,
	limits: &Limits) -> (Rectangle, Vec<Node>) {
		// limits, but without any max width
		let limits = Limits::new(
			limits.min(),
			Size::new(f32::INFINITY, limits.max().height)
		);

		let mut width: f32 = 0.0;
		let mut height: f32 = 0.0;

		let mut nodes: Vec<Node> = Vec::with_capacity(self.children.len());
		nodes.resize(self.children.len(), Node::default());

		for (i, (element, tree)) in self.children.iter_mut().zip(&mut tree.children).enumerate() {
			nodes[i] = element
				.as_widget_mut()
				.layout(tree, renderer, &limits)
				.move_to((width, 0.0));
			width += nodes[i].size().width;
			height = height.max(nodes[i].size().height);
		}

		(Rectangle::with_size(Size::new(width, height)), nodes)
	}

	/// Get the pixel width of a single character, or `None` if the current line is empty. (There
	/// has to be a better way to do this)
	fn char_width(&self, root_layout: &Layout) -> Option<f32> {
		// blank line?
		if self.spans[0].len == 0 {
			None
		} else {
			// this feels janktastic but it works.
			// SAFETY: always have children
			Some(root_layout.children().nth(0).unwrap().bounds().width / self.spans[0].len as f32)
		}
	}

	// --------------------------------------------------------------------------------------------
	// Message stuff

	fn get_operand_loc(&self, child_idx: ChildIdx) -> OperandLocation {
		match self.kind {
			LineKind::Code { bb_ea, instn, .. } => {
				if let Some(opn) = self.spans[child_idx.0].opn {
					return OperandLocation { bb_ea, instn, opn };
				}
				panic!("get_operand_loc called on a child with no operand index");
			}
			_ => panic!("get_operand_loc called on a non-code line"),
		}
	}

	fn publish_child_message<F>(&self, child_idx: ChildIdx,
	shell: &mut Shell<CodeViewMessage>, msgfn: F)
	where
		F: FnOnce(OperandLocation) -> CodeViewMessage,
	{
		#[allow(clippy::single_match)]
		match self.kind {
			LineKind::Code { bb_ea, instn, .. } => {
				let Some(opn) = self.spans[child_idx.0].opn else {
					panic!("publish_child_message called on a child with no operand index");
				};

				let loc = OperandLocation { bb_ea, instn, opn };
				shell.publish(msgfn(loc));
			}
			// right now, nothing. in future, might be e.g. data line which contains a reference
			_ => {}
		}
	}

	// --------------------------------------------------------------------------------------------
	// Cursor stuff

	/// Get the index of the child, if any, under the given `Point`.
	///
	/// Panics if `position` is not in `layout`
	fn child_at_position(&self, position: Point, layout: &Layout) -> Option<ChildIdx> {
		assert!(layout.bounds().contains(position));

		for (i, layout) in layout.children().enumerate() {
			if layout.bounds().contains(position) {
				return Some(ChildIdx(i))
			}
		}
		None
	}

	/// Get the index of the child, if any, at the given character index.
	fn child_at_char_index(&self, char_idx: usize) -> Option<ChildIdx> {
		let child_idx = match self.spans.binary_search_by_key(&char_idx, |span| span.start) {
			Ok(child_idx) => child_idx,
			Err(would_be) => would_be - 1,
		};

		if child_idx < self.spans.len() {
			Some(ChildIdx(child_idx))
		} else {
			None
		}
	}

	/// Given a `Point` inside this line's layout, compute the character index and cursor rectangle
	/// to place the cursor at the character under that point. If `position` is past the last
	/// character on the line, places the cursor immediately after the last character.
	///
	/// Panics if `position` is not in `layout`
	fn position_to_cursor(&self, position: Point, layout: &Layout) -> LineCursor {
		assert!(layout.bounds().contains(position));
		self.compute_line_cursor(layout, |bounds, char_width| {
			if let Some(char_width) = char_width {
				let position_in = position - bounds.position();
				let rightmost   = self.total_len as f32 * char_width;
				let xpos        = position_in.x.min(rightmost);
				(xpos / char_width) as usize
			} else {
				// this happens on blank lines, currently.
				0
			}
		})
	}

	/// Tries to change `line_cursor.idx` by `delta`; returns `true` if the cursor moved.
	fn move_cursor(&self, line_cursor: &mut LineCursor, delta: isize, layout: &Layout) -> bool {
		let idx = ((line_cursor.idx as isize) + delta)
			.clamp(0, self.total_len as isize) as usize;

		if idx != line_cursor.idx {
			*line_cursor = self.compute_line_cursor(layout, |_, _| idx);
			true
		} else {
			false
		}
	}

	fn compute_line_cursor<F>(&self, layout: &Layout, idxfn: F) -> LineCursor
	where
		F: Fn(Rectangle, Option<f32>) -> usize,
	{
		let char_width  = self.char_width(layout);
		let bounds      = layout.bounds();
		let idx         = idxfn(bounds, char_width);
		let char_width  = char_width.unwrap_or(0.0);
		LineCursor {
			idx,
			bounds: Rectangle::new(
				bounds.position() + Vector::new((idx as f32) * char_width, 0.0),
				Size::new(char_width, bounds.height)
			)
		}
	}
}

// ------------------------------------------------------------------------------------------------
// State, LineCursor
// ------------------------------------------------------------------------------------------------

struct LineCursor {
	/// 0-based character index of where the cursor is on the line. NOTE: this may be >= the line's
	/// total length! in that case the cursor is after the printing characters.
	idx:    usize,

	/// rectangle to be drawn to represent the cursor.
	bounds: Rectangle,
}

struct State {
	content_bounds: Rectangle,
	layouts:        Vec<Node>,

	/// which child, if any, the mouse is hovering over.
	hovered_child:  Option<ChildIdx>,

	/// which child, if any, the user pressed the left mouse button on.
	pressed_child:  Option<ChildIdx>,

	/// the previous mouse click (used for detecting double-clicks).
	previous_click: Option<Click>,

	/// if the text cursor is on this line, where it is.
	line_cursor:    Option<LineCursor>,

	/// which child, if any, the text cursor is over.
	focused_child:  Option<ChildIdx>,
}

impl State {
	fn needs_layout(&self) -> bool {
		self.layouts.is_empty()
	}
}

// ------------------------------------------------------------------------------------------------
// Widget implementation
// ------------------------------------------------------------------------------------------------

impl Widget<CodeViewMessage, iced::Theme, iced::Renderer> for CodeLine<'_> {
	fn children(&self) -> Vec<Tree> {
		self.children.iter().map(Tree::new).collect()
	}

	fn diff(&self, tree: &mut Tree) {
		tree.diff_children(&self.children);
	}

	fn tag(&self) -> Tag {
		Tag::of::<State>()
	}

	fn state(&self) -> TreeState {
		TreeState::new(State {
			content_bounds: Rectangle::with_size(Size::ZERO),
			layouts:        vec![],
			hovered_child:  None,
			pressed_child:  None,
			previous_click: None,
			line_cursor:    None,
			focused_child:  None,
		})
	}

	fn size(&self) -> Size<Length> {
		Size {
			width: self.width,
			height: self.height,
		}
	}

	fn layout(
		&mut self,
		tree: &mut Tree,
		renderer: &iced::Renderer,
		limits: &Limits
	) -> Node {
		let state = tree.state.downcast_ref::<State>();

		if state.needs_layout() {
			let (bounds, layouts) = self.layout_children(tree, renderer, limits);
			let state = tree.state.downcast_mut::<State>();
			state.content_bounds = bounds;
			state.layouts = layouts;
		}

		let state = tree.state.downcast_ref::<State>();
		let size = limits.resolve(Length::Fill, Length::Fill, state.content_bounds.size());
		Node::with_children(size, state.layouts.clone())
	}

	fn operate(
		&mut self,
		_tree: &mut Tree,
		layout: Layout<'_>,
		_renderer: &iced::Renderer,
		operation: &mut dyn widget::Operation,
	) {
		operation.container(None, layout.bounds());

		// In the future, if the children need to be operated on, here it is
		// operation.traverse(&mut |operation| {
		// 	self.children
		// 		.iter_mut()
		// 		.zip(&mut tree.children)
		// 		.zip(layout.children())
		// 		.for_each(|((child, state), layout)| {
		// 			child.as_widget_mut() .operate(state, layout, renderer, operation);
		// 		});
		// });
	}

	fn update(
		&mut self,
		tree: &mut Tree,
		event: &Event,
		layout: Layout<'_>,
		cursor: Cursor,
		_renderer: &iced::Renderer,
		_clipboard: &mut dyn Clipboard,
		shell: &mut Shell<'_, CodeViewMessage>,
		_viewport: &Rectangle,
	) {
		let old_hovered_child = tree.state.downcast_ref::<State>().hovered_child;
		let old_operand_loc = old_hovered_child.map(|child| self.get_operand_loc(child));
		let position_over = cursor.position_over(layout.bounds());

		// 1. see if mouse cursor is over
		if let Some(position) = position_over {
			// 2. if it is, see which child it's over
			tree.state.downcast_mut::<State>().hovered_child = self
				.child_at_position(position, &layout)
				.and_then(|child_idx| {
					if self.spans[child_idx.0].opn.is_some() {
						Some(child_idx)
					} else {
						None
					}
				});
		} else {
			tree.state.downcast_mut::<State>().hovered_child = None;
		}

		// 3. from that, look at self.spans to see if we should emit messages
		//    - hover messages (need to remember last-hovered span)
		//    - click messages
		//    - double-click messages
		let state = tree.state.downcast_ref::<State>();
		if old_hovered_child != state.hovered_child {
			shell.request_redraw();

			let new_operand_loc = state.hovered_child.map(|child| self.get_operand_loc(child));

			if old_operand_loc != new_operand_loc {

				if let Some(old_child) = old_hovered_child {
					self.publish_child_message(old_child, shell,
						|loc| CodeViewMessage::OperandHovered { loc, over: false });
				}

				if let Some(new_child) = state.hovered_child {
					self.publish_child_message(new_child, shell,
						|loc| CodeViewMessage::OperandHovered { loc, over: true });
				}
			}
		}

		let state = tree.state.downcast_mut::<State>();
		match event {
			Event::Mouse(MouseEvent::ButtonPressed(MouseButton::Left)) => {
				if state.hovered_child.is_some() {
					state.pressed_child = state.hovered_child;
					shell.capture_event();
				}
			}
			Event::Mouse(MouseEvent::ButtonReleased(MouseButton::Left)) => {
				if let Some(position) = cursor.position() {
					if position_over.is_some() {
						state.line_cursor = Some(self.position_to_cursor(position, &layout));
						shell.request_redraw();
					}

					if let Some(child_idx) = state.pressed_child
					&& Some(child_idx) == state.hovered_child {
						let new_click =
							Click::new(position, MouseButton::Left, state.previous_click);

						self.publish_child_message(child_idx, shell,
							|loc| CodeViewMessage::OperandClicked {
								loc,
								double: new_click.kind() == ClickKind::Double,
							});

						state.previous_click = Some(new_click);
						shell.capture_event();
					}
				}

				state.pressed_child = None;
			}
			Event::Keyboard(KeyboardEvent::KeyPressed { key, modifiers, .. })
			if *modifiers == KeyModifiers::NONE => {
				// TODO: this code is extremely similar to the hovering code above. possible to
				// abstract it?
				let state = tree.state.downcast_mut::<State>();
				let old_focused_child = state.focused_child;
				let old_operand_loc = old_focused_child.map(|child| self.get_operand_loc(child));

				if let Some(line_cursor) = &mut state.line_cursor {
					match key {
						// TODO: hold ctrl for moving left and right by span
						Key::Named(NamedKey::ArrowLeft) => {
							if self.move_cursor(line_cursor, -1, &layout) {
								shell.request_redraw();
							}
						}
						Key::Named(NamedKey::ArrowRight) => {
							if self.move_cursor(line_cursor, 1, &layout) {
								shell.request_redraw();
							}
						}
						_ => {}
					}
				}

				if let Some(LineCursor { idx, .. }) = state.line_cursor {
					state.focused_child = self
						.child_at_char_index(idx)
						.and_then(|child_idx| {
							if self.spans[child_idx.0].opn.is_some() {
								Some(child_idx)
							} else {
								None
							}
						});
				} else {
					state.focused_child = None;
				}

				if old_focused_child != state.focused_child {
					let new_operand_loc =
						state.focused_child.map(|child| self.get_operand_loc(child));

					if old_operand_loc != new_operand_loc {
						if let Some(old_child) = old_focused_child {
							self.publish_child_message(old_child, shell,
								|loc| CodeViewMessage::OperandFocused { loc, over: false });
						}

						if let Some(new_child) = state.focused_child {
							self.publish_child_message(new_child, shell,
								|loc| CodeViewMessage::OperandFocused { loc, over: true });
						}
					}
				}
			}
			_ => {}
		}

		// In the future, if the children need to be updated, here it is
		// for ((child, tree), layout) in self
		// 	.children
		// 	.iter_mut()
		// 	.zip(&mut tree.children)
		// 	.zip(layout.children())
		// {
		// 	child
		// 		.as_widget_mut()
		// 		.update(tree, event, layout, cursor, renderer, clipboard, shell, viewport);
		// }
	}

	fn mouse_interaction(
		&self,
		_tree: &Tree,
		_layout: Layout<'_>,
		_cursor: Cursor,
		_viewport: &Rectangle,
		_renderer: &iced::Renderer,
	) -> mouse::Interaction {
		mouse::Interaction::None
	}

	fn draw(
		&self,
		tree: &Tree,
		renderer: &mut iced::Renderer,
		theme: &Theme,
		style: &Style,
		layout: Layout,
		cursor: Cursor,
		viewport: &Rectangle
	) {
		if !layout.bounds().intersects(viewport) {
			return;
		}

		let state = tree.state.downcast_ref::<State>();
		let translation = layout.position() - Point::ORIGIN;

		if let Some(child_idx) = state.hovered_child {
			let bounds = state.layouts[child_idx.0].bounds();
			let bounds = Rectangle::new(
				bounds.position() - Vector::new(1.0, 1.0),
				bounds.size() + Size::new(2.0, 2.0),
			);

			// TODO: customizable color(s) for highlight background and border
			renderer.fill_quad(
				renderer::Quad {
					bounds: bounds + translation,
					border: iced::Border {
						color:  IcedColor::from_rgb8(0x90, 0x90, 0x90),
						width:  1.0,
						radius: iced::border::Radius::new(0.0),
					},
					..Default::default()
				},
				IcedColor::from_rgb8(0x20, 0x20, 0x20),
			);
		}

		if let Some(LineCursor { bounds, .. }) = state.line_cursor {
			let bounds = Rectangle::new(
				bounds.position() - Vector::new(1.0, 0.0),
				bounds.size() + Size::new(2.0, 0.0),
			);

			// TODO: customizable color for cursor
			renderer.fill_quad(
				renderer::Quad {
					bounds,
					border: iced::Border {
						color:  IcedColor::WHITE,
						width:  1.0,
						radius: iced::border::Radius::new(0.0),
					},
					..Default::default()
				},
				IcedColor::from_rgb8(0x00, 0x00, 0x00),
			);
		}

		for ((child, tree), layout) in self.children.iter()
			.zip(&tree.children)
			.zip(layout.children())
			.filter(|(_, layout)| layout.bounds().intersects(viewport))
		{
			child.as_widget().draw(tree, renderer, theme, style, layout, cursor, viewport);
		}
	}

	fn overlay<'b>(
		&'b mut self,
		_tree: &'b mut Tree,
		_layout: Layout<'b>,
		_renderer: &iced::Renderer,
		_viewport: &Rectangle,
		_translation: Vector,
	) -> Option<overlay::Element<'b, CodeViewMessage, iced::Theme, iced::Renderer>> {
		// could see this being used to pop up info tooltips
		None
		// overlay::from_children(
		// 	&mut self.children,
		// 	tree,
		// 	layout,
		// 	renderer,
		// 	viewport,
		// 	translation,
		// )
	}
}

impl<'a> From<CodeLine<'a>> for Element<'a, CodeViewMessage, iced::Theme, iced::Renderer> {
	fn from(code_line: CodeLine<'a>) -> Self {
		Self::new(code_line)
	}
}