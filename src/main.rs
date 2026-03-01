#![allow(unused)]

use std::collections::{ BTreeMap };
use std::ops::{ Bound };
use std::cell::{ RefCell };
use std::rc::{ Rc };

use iced::widget::text::{ Span as TextSpan };
use iced::{ Element, Font, color, Length, Border, Padding, Task };
use iced::font::{ Weight };
use iced::widget::{ pane_grid, text, column, row, span, container, scrollable, text::Rich, button,
	operation::{ self, AbsoluteOffset, RelativeOffset }, space };

use adi::{ EA, Span, SegId, SpanKind, PrintStyle };

use simplelog::*;
use log::*;
use better_panic::{ Settings as PanicSettings, Verbosity as PanicVerbosity };
use native_dialog::{ DialogBuilder };

use rand::prelude::*;

// ------------------------------------------------------------------------------------------------
// Modules
// ------------------------------------------------------------------------------------------------

mod backend;
mod sparse_list;
mod ui;

use backend::{ Backend, BackendEvent, SegmentChangedEvent };
use sparse_list::{ sparse_list, IContent, Change as ListChange };
use ui::*;

// ------------------------------------------------------------------------------------------------
// main
// ------------------------------------------------------------------------------------------------

fn main() -> Result<(), Box<dyn std::error::Error>> {
	setup_logging(LevelFilter::Debug)?;
	setup_panic();
	iced::application(AdiFE::init, AdiFE::update, AdiFE::view)
		.font(CONSOLAS_BYTES)
		.run()?;
	Ok(())
}

fn setup_logging(max_level: LevelFilter) -> Result<(), SetLoggerError> {
	let log_config = ConfigBuilder::new()
		.set_level_color(Level::Info, Some(simplelog::Color::Green))
		.set_level_color(Level::Debug, Some(simplelog::Color::Cyan))
		.set_level_color(Level::Trace, Some(simplelog::Color::White))
		.set_time_level(LevelFilter::Off)
		.set_thread_level(LevelFilter::Error)
		.set_target_level(LevelFilter::Off)
		.set_location_level(LevelFilter::Off)
		.set_level_padding(LevelPadding::Right)
		.add_filter_allow_str("adi_fe_iced")
		.build();
	TermLogger::init(max_level, log_config, TerminalMode::Mixed, ColorChoice::Always)
}

fn setup_panic() {
	PanicSettings::new()
		.lineno_suffix(true)
		.most_recent_first(false)
		.verbosity(PanicVerbosity::Full)
	.install();
}

// ------------------------------------------------------------------------------------------------
// Font
// ------------------------------------------------------------------------------------------------

const CONSOLAS_BYTES: &[u8] = include_bytes!("../resources/consolab.ttf");
const CONSOLAS_FONT: Font = Font::with_name("Consolas");

trait FontEx {
	fn bold(&self) -> Font;
}

impl FontEx for Font {
	fn bold(&self) -> Font {
		Font {
			weight: Weight::Bold,
			..*self
		}
	}
}

// ------------------------------------------------------------------------------------------------
// Message
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
enum Message {
	PaneDragged(pane_grid::DragEvent),
	PaneResized(pane_grid::ResizeEvent),
	OperandClicked { bb_ea: EA, instn: usize, opn: usize },
	JumpTo { ea: EA },
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
	fn into_message(self) -> Message {
		match self {
			CodeLink::Operand { bb_ea, instn, opn } => {
				Message::OperandClicked { bb_ea, instn, opn }
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
		_               => todo!("new PrintStyleEx was added!"),
	}
}

// ------------------------------------------------------------------------------------------------
// SpanRenderer
// ------------------------------------------------------------------------------------------------

struct SpanRenderer {
	spans: Vec<TextSpan<'static, CodeLink>>,
}

impl SpanRenderer {
	fn new() -> Self {
		Self { spans: vec![] }
	}

	fn finish(self) -> Vec<TextSpan<'static, CodeLink>> {
		self.spans
	}

	fn push(&mut self, s: impl Into<String>, color: u32) -> &mut Self {
		self.spans.push(self.make_span(s, color));
		self
	}

	fn push_link(&mut self, s: impl Into<String>, color: u32, link: CodeLink) -> &mut Self {
		self.spans.push(self.make_span(s, color).link(link));
		self
	}

	fn make_span(&self, s: impl Into<String>, color: u32) -> TextSpan<'static, CodeLink> {
		span(s.into())
			.color(iced::Color::from_rgb8(
				((color >> 16) & 0xFF) as u8,
				((color >> 8) & 0xFF) as u8,
				(color & 0xFF) as u8)
			)
	}

	fn plain(&mut self, s: impl Into<String>) -> &mut Self {
		self.push(s, color_of(PrintStyleEx::Plain))
	}

	fn comment(&mut self, s: impl Into<String>) -> &mut Self {
		self.push(s, color_of(PrintStyle::Comment))
	}

	fn label(&mut self, s: impl Into<String>) -> &mut Self {
		self.push(s, color_of(PrintStyle::Label))
	}

	fn seg_name(&mut self, s: impl Into<String>) -> &mut Self {
		self.push(s, color_of(PrintStyleEx::SegName))
	}

	fn code_bytes(&mut self, s: impl Into<String>) -> &mut Self {
		self.push(s, color_of(PrintStyleEx::CodeBytes))
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
}

// ------------------------------------------------------------------------------------------------
// Rendering methods for various pieces of code
// ------------------------------------------------------------------------------------------------

impl FunctionData {
	fn render_to(self, r: &mut SpanRenderer) {
		if self.name.is_empty() {
			return;
		}

		r.comment(
			"; ------------------------------------------------------------------------------")
			.newline();

		if self.is_piece {
			r.comment(format!("; (Piece of function {})", self.name)).newline();
		} else {
			r.comment(format!("; Function {}", self.name)).newline();

			if !self.attrs.is_empty() {
				r.comment(format!("; Attributes: {}", self.attrs)).newline();
			}

			if !self.entrypoints.is_empty() {
				r.comment(format!("; Entry points: {}", self.entrypoints)).newline();
			}
		}
	}
}

struct CodeLabel(String);

impl CodeLabel {
	fn render_to(self, r: &mut SpanRenderer) {
		if !self.0.is_empty() {
			r.label(format!("                   {}", self.0)).plain(":").newline();
		}
	}
}

impl TextEA {
	fn render_to(self, r: &mut SpanRenderer) {
		r.seg_name(self.seg).plain(format!(":{}", self.offs));
	}
}

struct CodeBytes(String);

impl CodeBytes {
	fn render_to(self, r: &mut SpanRenderer) {
		r.code_bytes(format!(" {:8}     ", self.0));
	}
}

impl CodeText {
	fn render_to(self, r: &mut SpanRenderer, bb_ea: EA, instn: usize) {
		if let Some(opn) = self.opn {
			r.push_link(self.text, color_of(self.style), CodeLink::Operand {
				bb_ea,
				instn,
				opn: opn.into()
			});
		} else {
			r.push(self.text, color_of(self.style));
		}
	}
}

impl BasicBlockData {
	fn render(self) -> Vec<TextSpan<'static, CodeLink>> {
		let mut r = SpanRenderer::new();

		// TODO: inrefs
		// TODO: MMU state

		// function name, attrs, entry points
		self.func.render_to(&mut r);

		// label
		CodeLabel(self.label).render_to(&mut r);

		// lines of code
		for (i, line) in self.lines.into_iter().enumerate() {
			// seg:offs
			line.ea.render_to(&mut r);

			// bytes
			CodeBytes(line.bytes).render_to(&mut r);

			// mnemonic
			r.mnemonic(line.mnemonic).plain(" ");

			// operands
			for op in line.operands.into_iter() {
				op.render_to(&mut r, self.ea, i);
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
			// seg:offs
			line.ea.render_to(&mut r);

			// bytes
			r.unknown_bytes(format!(" {}", line.bytes)).newline();
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
				// TODO:
				let mut r = SpanRenderer::new();
				r.error("AAAAAAAA DATA UNIMPLEMENTED");
				r.finish()
			}
			CodeViewItem::Unknown(unk) => unk.render(),
		}
	}
}

// ------------------------------------------------------------------------------------------------
// NamesPane
// ------------------------------------------------------------------------------------------------

struct NamesPane {
	names: Vec<(EA, String)>,
}

impl NamesPane {
	fn new(backend: Rc<Backend>) -> Self {
		// TODO: keep the backend and dynamically get names
		Self { names: backend.get_all_names() }
	}

	fn view(&self) -> (Element<'_, Message>, String) {
		let ui = scrollable(column(self.names.iter().map(|(ea, name)| {
			button(text(name).font(CONSOLAS_FONT.bold()))
				.style(button::text)
				.on_press(Message::JumpTo { ea: *ea })
				.into()
		})).width(Length::Fill).padding(Padding::from([0, 10])));

		(ui.into(), "Names".into())
	}
}

// ------------------------------------------------------------------------------------------------
// SegmentView
// ------------------------------------------------------------------------------------------------

struct SegmentView {
	backend: Rc<Backend>,
	seg:     SegId,
	changes: RefCell<Vec<ListChange>>,
}

impl SegmentView {
	fn new(seg: SegId, backend: Rc<Backend>) -> Self {
		Self {
			backend,
			seg,
			changes: RefCell::new(Vec::new()),
		}
	}

	fn render_span(&self, ea: EA) -> CodeViewItem {
		self.backend.get_rendered_span(ea)
	}

	fn segid(&self) -> SegId {
		self.seg
	}

	fn ea_after(&self, ea: EA) -> Option<EA> {
		self.backend.get_span_after(ea).map(|span| span.start())
	}

	fn ea_before(&self, ea: EA) -> Option<EA> {
		self.backend.get_span_before(ea).map(|span| span.start())
	}

	// fn insert(&mut self, idx: usize, val: Span) -> Option<Span> {
	// 	let ret = self.spans.insert(idx, val);

	// 	match ret {
	// 		None => self.changes.borrow_mut().push(ListChange::Added { idx }),
	// 		Some(ref old) if *old != val =>
	// 			self.changes.borrow_mut().push(ListChange::Changed { idx }),
	// 		_ => {}
	// 	}

	// 	ret
	// }

	// fn remove(&mut self, idx: usize) -> bool {
	// 	let ret = self.spans.remove(&idx).is_some();

	// 	if ret {
	// 		self.changes.borrow_mut().push(ListChange::Removed { idx });
	// 	}

	// 	ret
	// }
}

impl<'a> IContent<'a, EA> for SegmentView {
	fn len(&self) -> usize {
		self.backend.get_num_spans(self.seg)
	}

	fn first_index(&self) -> Option<usize> {
		// by definition
		Some(0)
	}

	fn last_index(&self) -> Option<usize> {
		Some(self.backend.get_last_span_offset(self.seg))
	}

	fn get(&self, idx: usize) -> Option<EA> {
		Some(self.backend.get_span(EA::new(self.seg, idx)).start())
	}

	fn items_before(&'a self, idx: usize)
	-> Box<dyn Iterator<Item = (usize, EA)> + 'a> {
		Box::new(SpansBefore { seg: self, ea: EA::new(self.seg, idx) })
	}

	fn items_after(&'a self, idx: usize)
	-> Box<dyn Iterator<Item = (usize, EA)> + 'a> {
		Box::new(SpansAfter { seg: self, ea: EA::new(self.seg, idx) })
	}

	// TODO: changes!!!!!
	fn changes(&self) -> Vec<ListChange> {
		self.changes.take()
	}
}

struct SpansAfter<'a> {
	seg: &'a SegmentView,
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
	seg: &'a SegmentView,
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

// ------------------------------------------------------------------------------------------------
// CodePane
// ------------------------------------------------------------------------------------------------

struct CodePane {
	seg: SegmentView,
}

impl CodePane {
	const LIST_ID: &str = "panes.code.list";

	fn new(seg: SegId, backend: Rc<Backend>) -> Self {
		Self {
			seg: SegmentView::new(seg, backend),
		}
	}

	fn set_segment(&mut self, seg: SegId) {
		self.seg = SegmentView::new(seg, self.seg.backend.clone());
	}

	fn view(&self) -> (Element<'_, Message>, String) {
		let list = sparse_list(
			&self.seg,
			|_, ea: EA| {
				container(Rich::with_spans(self.seg.render_span(ea).render())
					.on_link_click(CodeLink::into_message)
					.font(CONSOLAS_FONT.bold())
					.wrapping(iced::widget::text::Wrapping::None)
				)
				.width(Length::Fill)
				// .style(move |_theme| {
				// 	container::Style::default().border(
				// 		Border::default().color(color!(0xFFFFFF)).width(0.3))
				// })
				.into()
			}).id(Self::LIST_ID);

		let ui = container(list)
		.width(Length::Fill)
		.height(Length::Fill)
		.padding(Padding::from([0, 10]))
		.style(move |_theme| {
			container::Style::default().background(color!(0x101010))
		});

		(ui.into(), "Code".into())
	}
}

// ------------------------------------------------------------------------------------------------
// PaneState
// ------------------------------------------------------------------------------------------------

enum PaneState {
	Names(NamesPane),
	Code(CodePane),
}

impl PaneState {
	fn new_names(backend: Rc<Backend>) -> Self {
		Self::Names(NamesPane::new(backend))
	}

	fn new_code(seg: SegId, backend: Rc<Backend>) -> Self {
		Self::Code(CodePane::new(seg, backend))
	}

	fn view(&self) -> (Element<'_, Message>, String) {
		match self {
			PaneState::Names(n) => n.view(),
			PaneState::Code(c)  => c.view(),
		}
	}

	fn as_code(&self) -> &CodePane {
		match self {
			PaneState::Code(c) => c,
			_ => panic!(),
		}
	}

	fn as_code_mut(&mut self) -> &mut CodePane {
		match self {
			PaneState::Code(c) => c,
			_ => panic!(),
		}
	}
}

// ------------------------------------------------------------------------------------------------
// AdiFE
// ------------------------------------------------------------------------------------------------

struct AdiFE {
	backend: Rc<Backend>,
	panes: pane_grid::State<PaneState>,
	#[allow(dead_code)]
	name_pane: pane_grid::Pane,
	code_pane: pane_grid::Pane,
}

fn create_backend() -> Rc<Backend> {
	Rc::new(loop {
		let image = loop {
			// TODO: temporary
			let path = "/Users/me/src/re/adi/tests/data/smb.nes";
			match adi::Image::new_from_file(path) {
				Ok(image) => break image,
				Err(e) => {
					error!("Could not open {:?}: {}", path, e);
					std::process::exit(1);
				}
			}
			/*let path = DialogBuilder::file()
				.set_location("~/src/re/adi/tests/data")
				.open_single_file()
				.show()
				.unwrap();

			match path {
				Some(path) => {
					match adi::Image::new_from_file(&path) {
						Ok(image) => break image,
						Err(e) => {
							error!("Could not open {:?}: {}", path, e);
						}
					}
				}
				None => std::process::exit(1),
			}*/
		};

		info!("opened image {}", image.name());

		match Backend::on_new_thread(image) {
			Ok(backend) => break backend,
			Err(e) => error!("Could not analyze {}", e),
		}
	})
}

impl AdiFE {
	fn init() -> Self {
		AdiFE::new(create_backend())
	}

	fn new(backend: Rc<Backend>) -> Self {
		let (mut panes, name_pane) = pane_grid::State::new(PaneState::new_names(backend.clone()));
		let (code_pane, split) = panes.split(
			pane_grid::Axis::Vertical, name_pane, PaneState::new_code(
				SegId(3), // TODO: temporary
				backend.clone())).unwrap();
		panes.resize(split, 0.2);

		Self { backend: backend.clone(), panes, name_pane, code_pane }
	}

	fn code_pane(&self) -> &CodePane {
		self.panes.get(self.code_pane).unwrap().as_code()
	}

	fn code_pane_mut(&mut self) -> &mut CodePane {
		self.panes.get_mut(self.code_pane).unwrap().as_code_mut()
	}

	fn update(&mut self, message: Message) -> Task<Message> {
		match message {
			Message::PaneDragged(de) => {
				println!("TODO: dragged {:?}", de);
			}
			Message::PaneResized(pane_grid::ResizeEvent { split, ratio }) => {
				self.panes.resize(split, ratio);
			}
			Message::OperandClicked { bb_ea, instn, opn } => {
				println!("TODO: clicked operand {} of instruction {} in BB {:?}",
					opn, instn, bb_ea);
			}
			Message::JumpTo { ea } => {
				let code_pane = self.code_pane_mut();

				if code_pane.seg.segid() != ea.seg() {
					code_pane.set_segment(ea.seg());
				}

				return operation::scroll_to(CodePane::LIST_ID, AbsoluteOffset {
					y: Some(f32::from_bits(ea.offs() as u32)), // item index
					x: Some(80.0),                             // pixel offset from top
				});
			}
			Message::JumpToTop =>  {
				return operation::snap_to(CodePane::LIST_ID, RelativeOffset {
					x: None,
					y: Some(0.0),
				});
			}
			Message::JumpToBottom =>  {
				return operation::snap_to(CodePane::LIST_ID, RelativeOffset {
					x: None,
					y: Some(1.0),
				});
			}
			Message::Scroll { up } => {
				return operation::scroll_by(CodePane::LIST_ID, AbsoluteOffset {
					x: 0.0,
					y: if up { -20.0 } else { 20.0 },
				});
			}
		}

		Task::none()
	}

	fn view(&self) -> Element<'_, Message> {
		column![
			// trying to extract this callback into its own method is an exercise in frustration.
			// just leave it here unless you want to have the Worst Types and Where Clauses Ever.
			pane_grid(&self.panes, |_pane, state, _is_maximized| {
				let (content, title) = state.view();
				let title = text(title).size(20).font(Font::DEFAULT.bold());

				pane_grid::Content::new(content)
					.title_bar(pane_grid::TitleBar::new(title).padding(10))
			})
			.on_drag(Message::PaneDragged)
			.on_resize(10, Message::PaneResized)
			.min_size(200),

			row![
				button("top").on_press(Message::JumpToTop),
				space().width(10),
				button("bottom").on_press(Message::JumpToBottom),
				space().width(10),
				button("^").on_press(Message::Scroll { up: true }),
				space().width(10),
				button("v").on_press(Message::Scroll { up: false }),
			]
		].into()
	}
}