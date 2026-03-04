use std::cell::{ RefCell };
use std::rc::{ Rc };

use iced_core::{
	Widget, Layout, Rectangle,
	widget::{ Tree },
	mouse::{ Cursor },
	layout::{ Limits, Node },
	renderer::{ Style },
};

use iced::{
	Element, Color as IcedColor, color, Size, Length, Theme,
	widget::{ Column, text, mouse_area, row, },
};

use adi::{ EA, SegId, PrintStyle };

use crate::{ CONSOLAS_FONT_BOLD };
use crate::backend::{ Backend, SegmentChangedEvent };
use crate::ui::*;
use crate::widgets::sparse_list::{ sparse_list, IContent, Change as ListChange };

// ------------------------------------------------------------------------------------------------
// OperandLocation
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OperandLocation {
	pub(crate) bb_ea: EA,
	pub(crate) instn: usize,
	pub(crate) opn: u8,
}

// ------------------------------------------------------------------------------------------------
// CodeViewMessage
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub(crate) enum CodeViewMessage {
	OperandHovered { loc: OperandLocation, over: bool },
	OperandClicked { loc: OperandLocation },
	JumpTo { ea: EA },
	SwitchSegment { id: SegId },
	JumpToTop,
	JumpToBottom,
	Scroll { up: bool },
}

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
struct CodeLine<'a> {
	width:   Length,
	height:  Length,
	children: Vec<Element<'a, CodeViewMessage>>,

	ea:      EA,
	text_ea: Option<ChildIdx>,
	kind:    LineKind,
}

#[allow(unused)]
impl<'a> CodeLine<'a> {
	fn new_blank(ea: EA) -> Self {
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

	fn new_error(ea: EA, text_ea: TextEA, message: String) -> Self {
		let children = vec![
			textea(text_ea),                        // 0
			codetext(message, PrintStyleEx::Error), // 1
		];

		Self {
			width: Length::Shrink,
			height: Length::Shrink,
			children,
			ea,
			text_ea: Some(ChildIdx(0)),
			kind: LineKind::Error { message: ChildIdx(1) },
		}.adjust_size()
	}

	fn new_comment(ea: EA, comment: String) -> Self {
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

	fn new_label(ea: EA, label: String) -> Self {
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

	fn new_code(ea: EA, text_ea: TextEA, bb_ea: EA, instn: usize, code_bytes: String,
	mnemonic: String, operands: Vec<CodeOpData>) -> Self {
		let code_bytes = format!("{:8}     ", code_bytes);
		let mut children = vec![
			textea(text_ea),                                   // 0
			codetext(code_bytes, PrintStyleEx::CodeBytes),     // 1
			codetext(mnemonic,   PrintStyle::Mnemonic).into(), // 2
		];

		children.extend(operands.iter().map(|op| {             // 3, 4, ...
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
			text_ea: Some(ChildIdx(0)),
			kind: LineKind::Code {
				bb_ea,
				instn,
				code_bytes: ChildIdx(1),
				mnemonic:   ChildIdx(2),
				operands: operands.into_iter().enumerate()
					.map(|(i, op)| (op, ChildIdx(3 + i))).collect()
			},
		}.adjust_size()
	}

	fn new_unknown(ea: EA, text_ea: TextEA, bytes: String) -> Self {
		let children = vec![
			textea(text_ea),                        // 0
			codetext(bytes, PrintStyleEx::Unknown), // 1
		];
		Self {
			width: Length::Shrink,
			height: Length::Shrink,
			children,
			ea,
			text_ea: Some(ChildIdx(0)),
			kind: LineKind::Unknown { bytes: ChildIdx(1) },
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
}

impl Widget<CodeViewMessage, iced::Theme, iced::Renderer> for CodeLine<'_> {
	fn children(&self) -> Vec<Tree> {
		self.children.iter().map(Tree::new).collect()
	}

	fn diff(&self, tree: &mut Tree) {
		tree.diff_children(&self.children);
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
		resolve(
			renderer,
			limits,
			self.width,
			self.height,
			&mut self.children,
			&mut tree.children,
		)
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
					child
						.as_widget_mut()
						.operate(state, layout, renderer, operation);
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

pub fn resolve(
	renderer: &iced::Renderer,
	limits: &Limits,
	width: Length,
	height: Length,
	items: &mut [Element<'_, CodeViewMessage, iced::Theme, iced::Renderer>],
	trees: &mut [Tree],
) -> Node
{
	// TODO: there has to be an easier way to do this - this was adapted from the original
	// iced::layout::flex::resolve which supported all kinds of shit which isn't needed here,
	// but I'm too lazy to figure it out rn
	let limits = limits.width(width).height(height);
	let max_cross = limits.max().height;

	let (main_compress, cross_compress) = {
		let compression = limits.compression();
		(compression.width, compression.height)
	};

	let compression = {
		Size::new(main_compress, false)
	};

	let mut fill_main_sum = 0;
	let mut some_fill_cross = false;
	let mut cross = if cross_compress { 0.0 } else { max_cross };
	let mut available = limits.max().width;

	let mut nodes: Vec<Node> = Vec::with_capacity(items.len());
	nodes.resize(items.len(), Node::default());

	// FIRST PASS
	// We lay out non-fluid elements in the main axis.
	// If we need to compress the cross axis, then we skip any of these elements
	// that are also fluid in the cross axis.
	for (i, (child, tree)) in items.iter_mut().zip(trees.iter_mut()).enumerate() {
		let (fill_main_factor, fill_cross_factor) = {
			let size = child.as_widget().size();
			(size.width.fill_factor(), size.height.fill_factor())
		};

		if (main_compress || fill_main_factor == 0) && (!cross_compress || fill_cross_factor == 0) {
			let (max_width, max_height) = (
				available,
				if fill_cross_factor == 0 {
					max_cross
				} else {
					cross
				},
			);

			let child_limits =
				Limits::with_compression(Size::ZERO, Size::new(max_width, max_height), compression);

			let layout = child.as_widget_mut().layout(tree, renderer, &child_limits);
			let size = layout.size();

			available -= size.width;
			cross = cross.max(size.height);

			nodes[i] = layout;
		} else {
			fill_main_sum += fill_main_factor;
			some_fill_cross = some_fill_cross || fill_cross_factor != 0;
		}
	}

	// SECOND PASS (conditional)
	// If we must compress the cross axis and there are fluid elements in the
	// cross axis, we lay out any of these elements that are also non-fluid in
	// the main axis (i.e. the ones we deliberately skipped in the first pass).
	//
	// We use the maximum cross length obtained in the first pass as the maximum
	// cross limit.
	//
	// We can defer the layout of any elements that have a fixed size in the main axis,
	// allowing them to use the cross calculations of the next pass.
	if cross_compress && some_fill_cross {
		for (i, (child, tree)) in items.iter_mut().zip(trees.iter_mut()).enumerate() {
			let (main_size, cross_size) = {
				let size = child.as_widget().size();
				(size.width, size.height)
			};

			if (main_compress || main_size.fill_factor() == 0) && cross_size.fill_factor() != 0 {
				if let Length::Fixed(main) = main_size {
					available -= main;
					continue;
				}

				let (max_width, max_height) = (available, cross);

				let child_limits = Limits::with_compression(
					Size::ZERO,
					Size::new(max_width, max_height),
					compression,
				);

				let layout = child.as_widget_mut().layout(tree, renderer, &child_limits);
				let size = layout.size();

				available -= size.width;
				cross = cross.max(size.height);

				nodes[i] = layout;
			}
		}
	}

	let remaining = available.max(0.0);

	// THIRD PASS (conditional)
	// We lay out the elements that are fluid in the main axis.
	// We use the remaining space to evenly allocate space based on fill factors.
	if !main_compress {
		for (i, (child, tree)) in items.iter_mut().zip(trees.iter_mut()).enumerate() {
			let (fill_main_factor, fill_cross_factor) = {
				let size = child.as_widget().size();

				(size.width.fill_factor(), size.height.fill_factor())
			};

			if fill_main_factor != 0 {
				let max_main = remaining * fill_main_factor as f32 / fill_main_sum as f32;

				let max_main = if max_main.is_nan() {
					f32::INFINITY
				} else {
					max_main
				};

				let min_main = if max_main.is_infinite() {
					0.0
				} else {
					max_main
				};

				let (max_width, max_height) = (
					max_main,
					if fill_cross_factor == 0 {
						max_cross
					} else {
						cross
					},
				);

				let child_limits = Limits::with_compression(
					Size::new(min_main, 0.0),
					Size::new(max_width, max_height),
					compression,
				);

				let layout = child.as_widget_mut().layout(tree, renderer, &child_limits);
				cross = cross.max(layout.size().height);

				nodes[i] = layout;
			}
		}
	}

	// FOURTH PASS (conditional)
	// We lay out any elements that were deferred in the second pass.
	// These are elements that must be compressed in their cross axis and have
	// a fixed length in the main axis.
	if cross_compress && some_fill_cross {
		for (i, (child, tree)) in items.iter_mut().zip(trees).enumerate() {
			let (main_size, cross_size) = {
				let size = child.as_widget().size();

				(size.width, size.height)
			};

			if cross_size.fill_factor() != 0 {
				let Length::Fixed(main) = main_size else {
					continue;
				};

				let (max_width, max_height) = (main, cross);

				let child_limits = Limits::new(Size::ZERO, Size::new(max_width, max_height));

				let layout = child.as_widget_mut().layout(tree, renderer, &child_limits);
				let size = layout.size();

				cross = cross.max(size.height);

				nodes[i] = layout;
			}
		}
	}

	let mut main = 0.0;

	// FIFTH PASS
	// We align all the laid out nodes in the cross axis, if needed.
	for node in nodes.iter_mut() {
		node.move_to_mut(iced::Point::new(main, 0.0));
		node.align_mut(iced::Alignment::Start, iced::Alignment::Start, Size::new(0.0, cross));
		main += node.size().width;
	}

	let size = limits.resolve(width, height, Size::new(main, cross));
	Node::with_children(size, nodes)
}

impl<'a> From<CodeLine<'a>> for Element<'a, CodeViewMessage, iced::Theme, iced::Renderer> {
	fn from(code_line: CodeLine<'a>) -> Self {
		Self::new(code_line)
	}
}

// ------------------------------------------------------------------------------------------------
// Rendering helpers
// ------------------------------------------------------------------------------------------------

fn codetext(s: impl Into<String>, style: impl Into<PrintStyleEx>)
-> Element<'static, CodeViewMessage> {
	text(s.into())
		.font(CONSOLAS_FONT_BOLD)
		.color(color_of(style.into()))
		.into()
}

fn textea(ea: TextEA) -> Element<'static, CodeViewMessage> {
	row![
		codetext(ea.seg,                   PrintStyleEx::SegName),
		codetext(format!(":{} ", ea.offs), PrintStyleEx::Plain),
	].into()
}

// ------------------------------------------------------------------------------------------------
// CodeViewRenderer
// ------------------------------------------------------------------------------------------------

struct CodeViewRenderer {
	lines: Vec<CodeLine<'static>>,
}

impl CodeViewRenderer {
	// --------------------------------------------------------------------------------------------
	// Lifecycle

	fn new() -> Self {
		Self { lines: vec![] }
	}

	fn finish(self) -> Vec<CodeLine<'static>> {
		self.lines
	}

	// --------------------------------------------------------------------------------------------
	// Rendering methods

	fn blank_line(&mut self, ea: EA) {
		self.lines.push(CodeLine::new_blank(ea));
	}

	fn error_line(&mut self, ea: EA, text_ea: TextEA, message: impl Into<String>) {
		self.lines.push(CodeLine::new_error(ea, text_ea, message.into()));
	}

	fn comment_line(&mut self, ea: EA, comment: impl Into<String>) {
		self.lines.push(CodeLine::new_comment(ea, comment.into()));
	}

	fn label_line(&mut self, ea: EA, label: String) {
		if !label.is_empty() {
			let label = format!("                   {}", label);
			self.lines.push(CodeLine::new_label(ea, label));
		}
	}

	fn code_line(&mut self, ea: EA, text_ea: TextEA, bb_ea: EA, instn: usize, code_bytes: String,
	mnemonic: String, operands: Vec<CodeOpData>) {
		self.lines.push(
			CodeLine::new_code(ea, text_ea, bb_ea, instn, code_bytes, mnemonic, operands));
	}

	fn unknown_line(&mut self, ea: EA, text_ea: TextEA, bytes: String) {
		self.lines.push(CodeLine::new_unknown(ea, text_ea, bytes));
	}

	fn func_data(&mut self, ea: EA, data: Option<FuncData>) {
		let Some(data) = data else { return; };

		self.comment_line(ea,
			"------------------------------------------------------------------------------");

		match data.kind {
			FuncDataKind::Piece => {
				self.comment_line(ea, format!("(Piece of function {})", data.name));
			}
			FuncDataKind::Head { attrs, entrypoints } => {
				self.comment_line(ea, format!("Function {}", data.name));

				if let Some(attrs) = attrs {
					self.comment_line(ea, format!("Attributes: {}", attrs));
				}

				if let Some(entrypoints) = entrypoints {
					self.comment_line(ea, format!("Entry points: {}", entrypoints));
				}
			}
		}
	}
}

// ------------------------------------------------------------------------------------------------
// Rendering methods for various pieces of code
// ------------------------------------------------------------------------------------------------

impl BasicBlockData {
	fn render(self) -> Vec<CodeLine<'static>> {
		let mut r = CodeViewRenderer::new();

		// TODO: inrefs
		// TODO: MMU state

		// SAFETY: lines is never empty
		let first_ea = &self.lines.first().unwrap().ea;
		let last_ea = self.lines.last().unwrap().ea.clone();
		r.func_data(first_ea.clone(), self.func);
		r.label_line(first_ea.clone(), self.label);

		for (instn, line) in self.lines.into_iter().enumerate() {
			r.code_line(line.ea, line.text_ea, self.ea, instn, line.bytes, line.mnemonic,
				line.operands);
			// TODO: outrefs
		}

		r.blank_line(last_ea);
		r.finish()
	}
}

impl UnknownData {
	fn render(self) -> Vec<CodeLine<'static>> {
		let mut r = CodeViewRenderer::new();

		// SAFETY: lines is never empty
		let last_ea = self.lines.last().unwrap().ea.clone();

		for line in self.lines.into_iter() {
			r.unknown_line(line.ea, line.text_ea, line.bytes);
		}

		r.blank_line(last_ea);
		r.finish()
	}
}

impl CodeViewItem {
	fn render(self) -> Element<'static, CodeViewMessage> {
		let lines = match self {
			CodeViewItem::BasicBlock(bb) => bb.render(),
			CodeViewItem::DataItem(ea, text_ea) => {
				// TODO: data rendering
				let mut r = CodeViewRenderer::new();
				r.error_line(ea, text_ea, "DATA UNIMPLEMENTED");
				r.finish()
			}
			CodeViewItem::Unknown(unk) => unk.render(),
		};

		Column::with_children(lines.into_iter().map(|line| line.into())).into()
	}
}

// ------------------------------------------------------------------------------------------------
// CodeView
// ------------------------------------------------------------------------------------------------

pub(crate) struct CodeView {
	backend: Rc<Backend>,
	id:      SegId,
	changes: RefCell<Vec<ListChange>>,
}

impl CodeView {
	pub(crate) fn new(id: SegId, backend: Rc<Backend>) -> Self {
		Self {
			backend,
			id,
			changes: RefCell::new(Vec::new()),
		}
	}

	fn render_span(&self, ea: EA) -> CodeViewItem {
		self.backend.get_rendered_span(ea)
	}

	pub(crate) fn segid(&self) -> SegId {
		self.id
	}

	fn ea_after(&self, ea: EA) -> Option<EA> {
		self.backend.get_span_after(ea).map(|span| span.start())
	}

	fn ea_before(&self, ea: EA) -> Option<EA> {
		self.backend.get_span_before(ea).map(|span| span.start())
	}

	pub(crate) fn dispatch_event(&self, ea: EA, ev: SegmentChangedEvent) {
		if ea.seg() == self.id {
			use SegmentChangedEvent::*;
			self.changes.borrow_mut().push(
				match ev {
					Add    => ListChange::Added   { idx: ea.offs() },
					Remove => ListChange::Removed { idx: ea.offs() },
					Change => ListChange::Changed { idx: ea.offs() },
				}
			);
		}
	}

	pub(crate) fn view(&self, id: &'static str) -> Element<'_, CodeViewMessage> {
		sparse_list(self, |_, ea: EA| self.render_span(ea).render())
			.id(id)
			.into()
	}
}

// just to keep my thoughts straight
type SegOffs = usize;

impl<'a> IContent<'a, EA> for CodeView {
	fn len(&self) -> usize {
		self.backend.get_num_spans(self.id)
	}

	fn first_index(&self) -> Option<SegOffs> {
		// by definition
		Some(0)
	}

	fn last_index(&self) -> Option<SegOffs> {
		Some(self.backend.get_last_span_offset(self.id))
	}

	fn get(&self, idx: SegOffs) -> Option<EA> {
		Some(self.backend.get_span(EA::new(self.id, idx)).start())
	}

	fn items_before(&'a self, idx: SegOffs)
	-> Box<dyn Iterator<Item = (SegOffs, EA)> + 'a> {
		Box::new(SpansBefore { seg: self, ea: EA::new(self.id, idx) })
	}

	fn items_after(&'a self, idx: SegOffs)
	-> Box<dyn Iterator<Item = (SegOffs, EA)> + 'a> {
		Box::new(SpansAfter { seg: self, ea: EA::new(self.id, idx) })
	}

	fn changes(&self) -> Vec<ListChange> {
		self.changes.take()
	}
}

struct SpansAfter<'a> {
	seg: &'a CodeView,
	ea:  EA,
}

impl<'a> Iterator for SpansAfter<'a> {
	type Item = (SegOffs, EA);

	fn next(&mut self) -> Option<Self::Item> {
		self.seg.ea_after(self.ea).map(|next_ea| {
			self.ea = next_ea;
			(next_ea.offs(), next_ea)
		})
	}
}

struct SpansBefore<'a> {
	seg: &'a CodeView,
	ea:  EA,
}

impl<'a> Iterator for SpansBefore<'a> {
	type Item = (SegOffs, EA);

	fn next(&mut self) -> Option<Self::Item> {
		self.seg.ea_before(self.ea).map(|next_ea| {
			self.ea = next_ea;
			(next_ea.offs(), next_ea)
		})
	}
}
