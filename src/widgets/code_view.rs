use std::cell::{ RefCell };
use std::rc::{ Rc };

use iced::{
	Element, Length, Color as IcedColor,
	widget::{
		span, container,
		text::{ Rich, Wrapping, Span as TextSpan },
	},
};

use adi::{ EA, SegId, PrintStyle };

use crate::{ CONSOLAS_FONT, FontEx };
use crate::backend::{ Backend, SegmentChangedEvent };
use crate::ui::*;
use crate::widgets::sparse_list::{ sparse_list, IContent, Change as ListChange };

// ------------------------------------------------------------------------------------------------
// CodeViewMessage
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub(crate) enum CodeViewMessage {
	OperandClicked { bb_ea: EA, instn: usize, opn: usize },
	JumpTo { ea: EA },
	SwitchSegment { id: SegId },
	JumpToTop,
	JumpToBottom,
	Scroll { up: bool },
}

// ------------------------------------------------------------------------------------------------
// CodeLink
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Copy, Clone)]
enum CodeLink {
	Operand { bb_ea: EA, instn: usize, opn: usize },
}

impl CodeLink {
	fn into_message(self) -> CodeViewMessage {
		match self {
			CodeLink::Operand { bb_ea, instn, opn } => {
				CodeViewMessage::OperandClicked { bb_ea, instn, opn }
			}
		}
	}
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

fn color_of(style: impl Into<PrintStyleEx>) -> u32 {
	use PrintStyle::*;
	use PrintStyleEx::*;
	match style.into() {
		// TODO: make colors configurable
		Plain           => 0xFFFFFF, // white
		SegName         => 0xFFFF00, // yellow
		CodeBytes       => 0x8080FF, // light blue
		Unknown         => 0xFF7F00, // orange
		Error           => 0xFF4040, // light red
		Adi(Mnemonic)   => 0xFF0000, // red
		Adi(Register)   => 0xFFFFFF, // white
		Adi(Number)     => 0x00FF00, // bright green
		Adi(Symbol)     => 0xFFFFFF, // white
		Adi(String)     => 0xFF7F00, // orange
		Adi(Comment)    => 0x00AF00, // dark green
		Adi(Refname)    => 0xFFFFB0, // light tan
		Adi(Label)      => 0xA06000, // light brown
		Adi(Operand(_)) => panic!("trying to get the color of an operand"),
		Adi(_)          => todo!("a new PrintStyle was added!"),
	}
}

// ------------------------------------------------------------------------------------------------
// SpanRenderer
// ------------------------------------------------------------------------------------------------

struct SpanRenderer {
	spans: Vec<TextSpan<'static, CodeLink>>,
}

impl SpanRenderer {
	// --------------------------------------------------------------------------------------------
	// Lifecycle

	fn new() -> Self {
		Self { spans: vec![] }
	}

	fn finish(self) -> Vec<TextSpan<'static, CodeLink>> {
		self.spans
	}

	// --------------------------------------------------------------------------------------------
	// Making and pushing spans

	fn make_span(&self, s: impl Into<String>, color: u32) -> TextSpan<'static, CodeLink> {
		span(s.into())
			.color(IcedColor::from_rgb8(
				((color >> 16) & 0xFF) as u8,
				((color >> 8) & 0xFF) as u8,
				(color & 0xFF) as u8))
	}

	fn push(&mut self, s: impl Into<String>, color: u32) -> &mut Self {
		self.spans.push(self.make_span(s, color));
		self
	}

	fn push_link(&mut self, s: impl Into<String>, color: u32, link: CodeLink) -> &mut Self {
		self.spans.push(self.make_span(s, color).link(link));
		self
	}

	// --------------------------------------------------------------------------------------------
	// Rendering methods

	fn plain(&mut self, s: impl Into<String>) -> &mut Self {
		self.push(s, color_of(PrintStyleEx::Plain))
	}

	fn comment(&mut self, s: impl Into<String>) -> &mut Self {
		self.push(s, color_of(PrintStyle::Comment))
	}

	fn seg_name(&mut self, s: impl Into<String>) -> &mut Self {
		self.push(s, color_of(PrintStyleEx::SegName))
	}

	fn mnemonic(&mut self, s: impl Into<String>) -> &mut Self {
		self.push(s, color_of(PrintStyle::Mnemonic))
	}

	fn unknown_bytes(&mut self, s: impl Into<String>) -> &mut Self {
		self.push(s, color_of(PrintStyleEx::Unknown))
	}

	fn error(&mut self, s: impl Into<String>) -> &mut Self {
		self.push(s, color_of(PrintStyleEx::Error))
	}

	fn newline(&mut self) -> &mut Self {
		self.plain("\n")
	}

	fn label(&mut self, s: String) -> &mut Self {
		if !s.is_empty() {
			let s = format!("                   {}", s);
			self.push(s, color_of(PrintStyle::Label)).plain(":").newline();
		}
		self
	}

	fn code_bytes(&mut self, bytes: String) -> &mut Self {
		self.push(format!(" {:8}     ", bytes), color_of(PrintStyleEx::CodeBytes))
	}

	fn func_data(&mut self, data: Option<FuncData>) -> &mut Self {
		let Some(data) = data else {
			return self;
		};

		self.comment(
			"; ------------------------------------------------------------------------------")
			.newline();

		match data.kind {
			FuncDataKind::Piece => {
				self.comment(format!("; (Piece of function {})", data.name)).newline();
			}
			FuncDataKind::Head { attrs, entrypoints } => {
				self.comment(format!("; Function {}", data.name)).newline();

				if let Some(attrs) = attrs {
					self.comment(format!("; Attributes: {}", attrs)).newline();
				}

				if let Some(entrypoints) = entrypoints {
					self.comment(format!("; Entry points: {}", entrypoints)).newline();
				}
			}
		}

		self
	}

	fn ea(&mut self, ea: TextEA) -> &mut Self {
		self.seg_name(ea.seg).plain(format!(":{}", ea.offs))
	}

	fn operand(&mut self, op: CodeOpData, bb_ea: EA, instn: usize) -> &mut Self {
		if let Some(opn) = op.opn {
			self.push_link(op.text, color_of(op.style), CodeLink::Operand {
				bb_ea,
				instn,
				opn: opn.into()
			})
		} else {
			self.push(op.text, color_of(op.style))
		}
	}
}

// ------------------------------------------------------------------------------------------------
// Rendering methods for various pieces of code
// ------------------------------------------------------------------------------------------------

impl BasicBlockData {
	fn render(self) -> Vec<TextSpan<'static, CodeLink>> {
		let mut r = SpanRenderer::new();

		// TODO: inrefs
		// TODO: MMU state

		r.func_data(self.func)
			.label(self.label);

		// lines of code
		for (i, line) in self.lines.into_iter().enumerate() {
			r.ea(line.ea)
				.code_bytes(line.bytes)
				.mnemonic(line.mnemonic)
				.plain(" ");

			for op in line.operands.into_iter() {
				r.operand(op, self.ea, i);
			}

			// TODO: outrefs
			r.newline();
		}

		r.newline();
		r.finish()
	}
}

impl UnknownData {
	fn render(self) -> Vec<TextSpan<'static, CodeLink>> {
		let mut r = SpanRenderer::new();

		for line in self.lines.into_iter() {
			r.ea(line.ea)
				.unknown_bytes(format!(" {}", line.bytes))
				.newline();
		}

		r.newline();
		r.finish()
	}
}

impl CodeViewItem {
	fn render(self) -> Vec<TextSpan<'static, CodeLink>> {
		match self {
			CodeViewItem::BasicBlock(bb) => bb.render(),
			CodeViewItem::DataItem => {
				// TODO: data rendering
				let mut r = SpanRenderer::new();
				r.error("AAAAAAAA DATA UNIMPLEMENTED");
				r.finish()
			}
			CodeViewItem::Unknown(unk) => unk.render(),
		}
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
			match ev {
				Add    => self.changes.borrow_mut().push(ListChange::Added   { idx: ea.offs() }),
				Remove => self.changes.borrow_mut().push(ListChange::Removed { idx: ea.offs() }),
				Change => self.changes.borrow_mut().push(ListChange::Changed { idx: ea.offs() }),
			}
		}
	}

	pub(crate) fn view(&self, id: &'static str) -> Element<'_, CodeViewMessage> {
		sparse_list(
			self,
			|_, ea: EA| {
				container(Rich::with_spans(self.render_span(ea).render())
					.on_link_click(CodeLink::into_message)
					.font(CONSOLAS_FONT.bold())
					.wrapping(Wrapping::None)
				)
				.width(Length::Fill)
				// .style(move |_theme| {
				// 	container::Style::default().border(
				// 		Border::default().color(color!(0xFFFFFF)).width(0.3))
				// })
				.into()
			}).id(id).into()
	}
}

impl<'a> IContent<'a, EA> for CodeView {
	fn len(&self) -> usize {
		self.backend.get_num_spans(self.id)
	}

	fn first_index(&self) -> Option<usize> {
		// by definition
		Some(0)
	}

	fn last_index(&self) -> Option<usize> {
		Some(self.backend.get_last_span_offset(self.id))
	}

	fn get(&self, idx: usize) -> Option<EA> {
		Some(self.backend.get_span(EA::new(self.id, idx)).start())
	}

	fn items_before(&'a self, idx: usize)
	-> Box<dyn Iterator<Item = (usize, EA)> + 'a> {
		Box::new(SpansBefore { seg: self, ea: EA::new(self.id, idx) })
	}

	fn items_after(&'a self, idx: usize)
	-> Box<dyn Iterator<Item = (usize, EA)> + 'a> {
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
	type Item = (usize, EA);

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
	type Item = (usize, EA);

	fn next(&mut self) -> Option<Self::Item> {
		self.seg.ea_before(self.ea).map(|next_ea| {
			self.ea = next_ea;
			(next_ea.offs(), next_ea)
		})
	}
}
