
use iced_core::{
	Widget, Layout, Rectangle,
	widget::{ Tree, tree::{ Tag, State as TreeState } },
	mouse::{ Cursor },
	layout::{ Limits, Node },
	renderer::{ Style },
};

use iced::{
	Element, Color as IcedColor, color, Size, Length, Theme,
	widget::{ text, mouse_area, },
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
// Rendering helpers
// ------------------------------------------------------------------------------------------------

fn codetext(s: impl Into<String>, style: impl Into<PrintStyleEx>)
-> Element<'static, CodeViewMessage> {
	text(s.into())
		.font(CONSOLAS_FONT_BOLD) // TODO: make font customizable
		.color(color_of(style.into()))
		.into()
}

// ------------------------------------------------------------------------------------------------
// State
// ------------------------------------------------------------------------------------------------

struct State {
	content_bounds: Rectangle,
	layouts:        Vec<Node>,
}

impl State {
	fn needs_layout(&self) -> bool {
		self.layouts.is_empty()
	}
}

// ------------------------------------------------------------------------------------------------
// LineKind, CodeLine
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

#[allow(unused)]
pub(crate) struct CodeLine<'a> {
	width:   Length,
	height:  Length,
	children: Vec<Element<'a, CodeViewMessage>>,

	ea:      EA,
	text_ea: Option<(ChildIdx, ChildIdx)>,
	kind:    LineKind,
}

#[allow(unused)]
impl<'a> CodeLine<'a> {
	pub(crate) fn new_blank(ea: EA) -> Self {
		let children = vec![codetext("", PrintStyleEx::Plain)];
		Self {
			width: Length::Shrink,
			height: Length::Shrink,
			children,
			ea,
			text_ea: None,
			kind: LineKind::Blank { dummy: ChildIdx(0) },
		}.adjust_size()
	}

	pub(crate) fn new_error(ea: EA, text_ea: TextEA, message: String) -> Self {
		let children = vec![
			codetext(text_ea.seg,                   PrintStyleEx::SegName), // 0
			codetext(format!(":{} ", text_ea.offs), PrintStyleEx::Plain),   // 1
			codetext(message,                       PrintStyleEx::Error),   // 2
		];

		Self {
			width: Length::Shrink,
			height: Length::Shrink,
			children,
			ea,
			text_ea: Some((ChildIdx(0), ChildIdx(1))),
			kind: LineKind::Error { message: ChildIdx(2) },
		}.adjust_size()
	}

	pub(crate) fn new_comment(ea: EA, comment: String) -> Self {
		let children = vec![
			codetext(format!("; {}", comment), PrintStyle::Comment), // 0
		];
		Self {
			width: Length::Shrink,
			height: Length::Shrink,
			children,
			ea,
			text_ea: None,
			kind: LineKind::Comment { comment: ChildIdx(0) },
		}.adjust_size()
	}

	pub(crate) fn new_label(ea: EA, label: String) -> Self {
		assert!(!label.is_empty());
		let children = vec![
			codetext(label, PrintStyle::Label),   // 0
			codetext(":",   PrintStyleEx::Plain), // 1
		];

		Self {
			width: Length::Shrink,
			height: Length::Shrink,
			children,
			ea,
			text_ea: None,
			kind: LineKind::Label { label: ChildIdx(0) },
		}.adjust_size()
	}

	pub(crate) fn new_code(ea: EA, text_ea: TextEA, bb_ea: EA, instn: usize, code_bytes: String,
	mnemonic: String, operands: Vec<CodeOpData>) -> Self {
		let code_bytes = format!("{:8}     ", code_bytes);
		let mut children = vec![
			codetext(text_ea.seg,                   PrintStyleEx::SegName),   // 0
			codetext(format!(":{} ", text_ea.offs), PrintStyleEx::Plain),     // 1
			codetext(code_bytes,                    PrintStyleEx::CodeBytes), // 2
			codetext(mnemonic,                      PrintStyle::Mnemonic),    // 3
		];

		children.extend(operands.iter().map(|op| {                            // 4, 5, ...
			match op.opn {
				Some(opn) => {
					let loc = OperandLocation { bb_ea, instn, opn };
					mouse_area(codetext(op.text.clone(), op.style))
						.on_enter(CodeViewMessage::OperandHovered { loc, over: true })
						.on_exit (CodeViewMessage::OperandHovered { loc, over: false })
						.on_press(CodeViewMessage::OperandClicked { loc })
						.into()
				}
				None => codetext(op.text.clone(), op.style),
			}
		}));

		Self {
			width: Length::Shrink,
			height: Length::Shrink,
			children,
			ea,
			text_ea: Some((ChildIdx(0), ChildIdx(1))),
			kind: LineKind::Code {
				bb_ea,
				instn,
				code_bytes: ChildIdx(2),
				mnemonic:   ChildIdx(3),
				operands: operands.into_iter().enumerate()
					.map(|(i, op)| (op, ChildIdx(4 + i))).collect()
			},
		}.adjust_size()
	}

	pub(crate) fn new_unknown(ea: EA, text_ea: TextEA, bytes: String) -> Self {
		let children = vec![
			codetext(text_ea.seg,                   PrintStyleEx::SegName), // 0
			codetext(format!(":{} ", text_ea.offs), PrintStyleEx::Plain),   // 1
			codetext(bytes,                         PrintStyleEx::Unknown), // 2
		];
		Self {
			width: Length::Shrink,
			height: Length::Shrink,
			children,
			ea,
			text_ea: Some((ChildIdx(0), ChildIdx(1))),
			kind: LineKind::Unknown { bytes: ChildIdx(2) },
		}.adjust_size()
	}

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
}

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
		let state = tree.state.downcast_mut::<State>();

		if state.needs_layout() {
			let (bounds, layouts) = self.layout_children(tree, renderer, limits);
			let state = tree.state.downcast_mut::<State>();
			state.content_bounds = bounds;
			state.layouts = layouts;
		}

		let state = tree.state.downcast_mut::<State>();
		let size = limits.resolve(Length::Fill, Length::Fill, state.content_bounds.size());
		Node::with_children(size, state.layouts.clone())
	}

	fn operate(
		&mut self,
		tree: &mut Tree,
		layout: Layout<'_>,
		renderer: &iced::Renderer,
		operation: &mut dyn iced_core::widget::Operation,
	) {
		operation.container(None, layout.bounds());
		operation.traverse(&mut |operation| {
			self.children
				.iter_mut()
				.zip(&mut tree.children)
				.zip(layout.children())
				.for_each(|((child, state), layout)| {
					child.as_widget_mut() .operate(state, layout, renderer, operation);
				});
		});
	}

	fn update(
		&mut self,
		tree: &mut Tree,
		event: &iced::Event,
		layout: Layout<'_>,
		cursor: iced::mouse::Cursor,
		renderer: &iced::Renderer,
		clipboard: &mut dyn iced_core::Clipboard,
		shell: &mut iced_core::Shell<'_, CodeViewMessage>,
		viewport: &Rectangle,
	) {
		for ((child, tree), layout) in self
			.children
			.iter_mut()
			.zip(&mut tree.children)
			.zip(layout.children())
		{
			child
				.as_widget_mut()
				.update(tree, event, layout, cursor, renderer, clipboard, shell, viewport);
		}
	}

	fn mouse_interaction(
		&self,
		tree: &Tree,
		layout: Layout<'_>,
		cursor: iced::mouse::Cursor,
		viewport: &Rectangle,
		renderer: &iced::Renderer,
	) -> iced::mouse::Interaction {
		self.children
			.iter()
			.zip(&tree.children)
			.zip(layout.children())
			.map(|((child, tree), layout)| {
				child
					.as_widget()
					.mouse_interaction(tree, layout, cursor, viewport, renderer)
			})
			.max()
			.unwrap_or_default()
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
		if layout.bounds().intersects(viewport) {
			for ((child, tree), layout) in self
				.children
				.iter()
				.zip(&tree.children)
				.zip(layout.children())
				.filter(|(_, layout)| layout.bounds().intersects(viewport))
			{
				child
					.as_widget()
					.draw(tree, renderer, theme, style, layout, cursor, viewport);
			}
		}
	}

	fn overlay<'b>(
		&'b mut self,
		tree: &'b mut Tree,
		layout: Layout<'b>,
		renderer: &iced::Renderer,
		viewport: &Rectangle,
		translation: iced::Vector,
	) -> Option<iced_core::overlay::Element<'b, CodeViewMessage, iced::Theme, iced::Renderer>> {
		iced_core::overlay::from_children(
			&mut self.children,
			tree,
			layout,
			renderer,
			viewport,
			translation,
		)
	}
}

impl<'a> From<CodeLine<'a>> for Element<'a, CodeViewMessage, iced::Theme, iced::Renderer> {
	fn from(code_line: CodeLine<'a>) -> Self {
		Self::new(code_line)
	}
}