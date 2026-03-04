use std::cell::{ RefCell };
use std::rc::{ Rc };

use iced::{
	Element, Color as IcedColor, color,
	widget::{
		Row, Column, text, row, mouse_area,
	},
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
	pub(crate) opn: usize,
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
		Adi(_)          => todo!("a new PrintStyle was added!"),
	}
}

// ------------------------------------------------------------------------------------------------
// LineKind, CodeLine
// ------------------------------------------------------------------------------------------------

enum LineKind {
	Blank,
	Error { message: String },
	Comment { comment: String },
	Label { label: String },
	Code {
		bb_ea: EA,
		instn: usize,
		code_bytes: String,
		mnemonic: String,
		operands: Vec<CodeOpData>
		// outrefs: String,
	},
	Unk { bytes: String },
	// TODO: data
}

struct CodeLine {
	ea: Option<TextEA>,
	kind: LineKind,
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

fn blank_line() -> Element<'static, CodeViewMessage> {
	codetext("", PrintStyleEx::Plain)
}

fn error_line(ea: TextEA, message: String) -> Element<'static, CodeViewMessage> {
	row![
		codetext(ea.seg,                   PrintStyleEx::SegName),
		codetext(format!(":{} ", ea.offs), PrintStyleEx::Plain),
		codetext(message,                  PrintStyleEx::Error),
	].into()
}

fn comment_line(comment: String) -> Element<'static, CodeViewMessage> {
	codetext(format!("; {}", comment), PrintStyle::Comment)
}

fn label_line(label: String) -> Element<'static, CodeViewMessage> {
	row![
		codetext(format!("                   {}", label), PrintStyle::Label),
		codetext(":",                                     PrintStyleEx::Plain),
	].into()
}

fn code_line(ea: TextEA, bb_ea: EA, instn: usize, code_bytes: String, mnemonic: String,
operands: Vec<CodeOpData>) -> Element<'static, CodeViewMessage> {
	let mut items = vec![
		codetext(ea.seg,                            PrintStyleEx::SegName),
		codetext(format!(":{}", ea.offs),           PrintStyleEx::Plain),
		codetext(format!(" {:8}     ", code_bytes), PrintStyleEx::CodeBytes),
		codetext(mnemonic,                          PrintStyle::Mnemonic),
	];

	for op in operands.into_iter() {
		if let Some(opn) = op.opn {
			items.push(
				mouse_area(codetext(op.text, op.style))
					.on_enter(CodeViewMessage::OperandHovered
						{ loc: OperandLocation { bb_ea, instn, opn: opn as usize }, over: true })
					.on_exit(CodeViewMessage::OperandHovered
						{ loc: OperandLocation { bb_ea, instn, opn: opn as usize }, over: false })
					.on_press(CodeViewMessage::OperandClicked
						{ loc: OperandLocation { bb_ea, instn, opn: opn as usize } })
					.into()
			);
		} else {
			items.push(codetext(op.text, op.style));
		}
	}

	Row::from_vec(items).into()
}

fn unknown_line(ea: TextEA, bytes: String) -> Element<'static, CodeViewMessage> {
	row![
		codetext(ea.seg,                  PrintStyleEx::SegName),
		codetext(format!(":{}", ea.offs), PrintStyleEx::Plain),
		codetext(bytes,                   PrintStyleEx::Unknown),
	].into()
}

// ------------------------------------------------------------------------------------------------
// CodeViewRenderer
// ------------------------------------------------------------------------------------------------

struct CodeViewRenderer {
	lines: Vec<Element<'static, CodeViewMessage>>,
}

impl CodeViewRenderer {
	// --------------------------------------------------------------------------------------------
	// Lifecycle

	fn new() -> Self {
		Self { lines: vec![] }
	}

	fn finish(self) -> Vec<Element<'static, CodeViewMessage>> {
		self.lines
	}

	// --------------------------------------------------------------------------------------------
	// Pushing lines

	fn push_line(&mut self, line: CodeLine) {
		use LineKind::*;
		let CodeLine { ea, kind } = line;

		match kind {
			Blank => {
				assert!(ea.is_none(), "blank line has spurious EA");
				self.lines.push(blank_line());
			}
			Error { message } => {
				let Some(ea) = ea else { panic!("error line missing EA"); };
				self.lines.push(error_line(ea, message));
			}
			Comment { comment } => {
				assert!(ea.is_none(), "comment line has spurious EA");
				self.lines.push(comment_line(comment));
			}
			Label { label } => {
				assert!(ea.is_none(), "label line has spurious EA");
				self.lines.push(label_line(label));
			}
			Code { bb_ea, instn, code_bytes, mnemonic, operands, /*outrefs*/ } => {
				let Some(ea) = ea else { panic!("code line missing EA"); };
				self.lines.push(code_line(ea, bb_ea, instn, code_bytes, mnemonic, operands));
			}
			Unk { bytes } => {
				let Some(ea) = ea else { panic!("unknown line missing EA"); };
				self.lines.push(unknown_line(ea, bytes));
			}
		}
	}

	// --------------------------------------------------------------------------------------------
	// Rendering methods

	fn blank_line(&mut self) {
		self.push_line(CodeLine {
			ea: None,
			kind: LineKind::Blank,
		});
	}

	fn error_line(&mut self, ea: TextEA, message: impl Into<String>) {
		self.push_line(CodeLine {
			ea: Some(ea),
			kind: LineKind::Error { message: message.into() },
		});
	}

	fn comment_line(&mut self, comment: impl Into<String>) {
		self.push_line(CodeLine {
			ea: None,
			kind: LineKind::Comment { comment: comment.into() },
		});
	}

	fn label_line(&mut self, label: String) {
		if !label.is_empty() {
			self.push_line(CodeLine {
				ea: None,
				kind: LineKind::Label { label },
			});
		}
	}

	fn code_line(&mut self, ea: TextEA, bb_ea: EA, instn: usize, code_bytes: String,
	mnemonic: String, operands: Vec<CodeOpData>) {
		self.push_line(CodeLine {
			ea: Some(ea),
			kind: LineKind::Code { bb_ea, instn, code_bytes, mnemonic, operands },
		});
	}

	fn unknown_line(&mut self, ea: TextEA, bytes: String) {
		self.push_line(CodeLine {
			ea: Some(ea),
			kind: LineKind::Unk { bytes }
		});
	}

	fn func_data(&mut self, data: Option<FuncData>) {
		let Some(data) = data else { return; };

		self.comment_line(
			"; ------------------------------------------------------------------------------");

		match data.kind {
			FuncDataKind::Piece => {
				self.comment_line(format!("; (Piece of function {})", data.name));
			}
			FuncDataKind::Head { attrs, entrypoints } => {
				self.comment_line(format!("; Function {}", data.name));

				if let Some(attrs) = attrs {
					self.comment_line(format!("; Attributes: {}", attrs));
				}

				if let Some(entrypoints) = entrypoints {
					self.comment_line(format!("; Entry points: {}", entrypoints));
				}
			}
		}
	}
}

// ------------------------------------------------------------------------------------------------
// Rendering methods for various pieces of code
// ------------------------------------------------------------------------------------------------

impl BasicBlockData {
	fn render(self) -> Vec<Element<'static, CodeViewMessage>> {
		let mut r = CodeViewRenderer::new();

		// TODO: inrefs
		// TODO: MMU state

		r.func_data(self.func);
		r.label_line(self.label);

		for (instn, line) in self.lines.into_iter().enumerate() {
			r.code_line(line.ea, self.ea, instn, line.bytes, line.mnemonic, line.operands);
			// TODO: outrefs
		}

		r.blank_line();
		r.finish()
	}
}

impl UnknownData {
	fn render(self) -> Vec<Element<'static, CodeViewMessage>> {
		let mut r = CodeViewRenderer::new();

		for line in self.lines.into_iter() {
			r.unknown_line(line.ea, format!(" {}", line.bytes));
		}

		r.blank_line();
		r.finish()
	}
}

impl CodeViewItem {
	fn render(self) -> Element<'static, CodeViewMessage> {
		let lines = match self {
			CodeViewItem::BasicBlock(bb) => bb.render(),
			CodeViewItem::DataItem(ea) => {
				// TODO: data rendering
				let mut r = CodeViewRenderer::new();
				r.error_line(ea, "AAAAAAAA DATA UNIMPLEMENTED");
				r.finish()
			}
			CodeViewItem::Unknown(unk) => unk.render(),
		};

		Column::from_vec(lines).into()
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
		sparse_list(self, |_, ea: EA| self.render_span(ea).render())
			.id(id)
			.into()
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
