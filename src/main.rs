#![allow(unused)]

use std::cell::{ RefCell };
use std::rc::{ Rc };

use iced::{
	Element, Font, Length, Padding, Task, Color as IcedColor, color, Subscription,
	font::{ Weight },
	time::{ self, milliseconds },
	widget::{
		column, row, span, container, scrollable, button, space, pick_list,

		pane_grid,
		pane_grid::{
			Pane,
			Axis as PaneAxis,
			Content as PaneContent,
			DragEvent as PaneDragEvent,
			ResizeEvent as PaneResizeEvent,
			State as PaneState,
			TitleBar as PaneTitleBar,
		},

		text,
		text::{ Rich, Wrapping, Span as TextSpan },

		operation::{ self, AbsoluteOffset, RelativeOffset },
	},
};

use adi::{ EA, SegId, PrintStyle, Image };

use simplelog::{ *, Color as SimpleLogColor };
use log::*;
use better_panic::{ Settings as PanicSettings, Verbosity as PanicVerbosity };
use native_dialog::{ DialogBuilder };

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
		.subscription(AdiFE::subscriptions)
		.run()?;
	Ok(())
}

fn setup_logging(max_level: LevelFilter) -> Result<(), SetLoggerError> {
	let log_config = ConfigBuilder::new()
		.set_level_color(Level::Info, Some(SimpleLogColor::Green))
		.set_level_color(Level::Debug, Some(SimpleLogColor::Cyan))
		.set_level_color(Level::Trace, Some(SimpleLogColor::White))
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
	PaneDragged(PaneDragEvent),
	PaneResized(PaneResizeEvent),
	OperandClicked { bb_ea: EA, instn: usize, opn: usize },
	JumpTo { ea: EA },
	SwitchSegment { id: SegId },
	JumpToTop,
	JumpToBottom,
	Scroll { up: bool },
	CheckForEvents,
	ForceAnalyze,
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

	fn func_data(&mut self, data: FunctionData) -> &mut Self {
		if data.name.is_empty() {
			return self;
		}

		self.comment(
			"; ------------------------------------------------------------------------------")
			.newline();

		if data.is_piece {
			self.comment(format!("; (Piece of function {})", data.name)).newline();
		} else {
			self.comment(format!("; Function {}", data.name)).newline();

			if !data.attrs.is_empty() {
				self.comment(format!("; Attributes: {}", data.attrs)).newline();
			}

			if !data.entrypoints.is_empty() {
				self.comment(format!("; Entry points: {}", data.entrypoints)).newline();
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
// NamesPane
// ------------------------------------------------------------------------------------------------

struct NamesPane {
	names: Vec<(EA, String)>,
}

impl NamesPane {
	fn new(backend: Rc<Backend>) -> Self {
		// TODO: keep the backend and dynamically get names... need some kind of listener in adi
		// to listen for name changes to do that
		Self { names: backend.get_all_names() }
	}

	fn view(&self) -> PaneContent<'_, Message> {
		let ui = scrollable(column(self.names.iter().map(|(ea, name)| {
			button(text(name).font(CONSOLAS_FONT.bold()))
				.style(button::text)
				.on_press(Message::JumpTo { ea: *ea })
				.into()
		})).width(Length::Fill).padding(Padding::from([0, 10])));

		let title = text("Names").size(20).font(Font::DEFAULT.bold());
		PaneContent::new(ui)
			.title_bar(PaneTitleBar::new(title).padding(10))
	}
}

// ------------------------------------------------------------------------------------------------
// SegmentView
// ------------------------------------------------------------------------------------------------

struct SegmentView {
	backend: Rc<Backend>,
	id:      SegId,
	changes: RefCell<Vec<ListChange>>,
}

impl SegmentView {
	fn new(id: SegId, backend: Rc<Backend>) -> Self {
		Self {
			backend,
			id,
			changes: RefCell::new(Vec::new()),
		}
	}

	fn render_span(&self, ea: EA) -> CodeViewItem {
		self.backend.get_rendered_span(ea)
	}

	fn id(&self) -> SegId {
		self.id
	}

	fn ea_after(&self, ea: EA) -> Option<EA> {
		self.backend.get_span_after(ea).map(|span| span.start())
	}

	fn ea_before(&self, ea: EA) -> Option<EA> {
		self.backend.get_span_before(ea).map(|span| span.start())
	}

	fn dispatch_event(&self, ea: EA, ev: SegmentChangedEvent) {
		if ea.seg() == self.id {
			use SegmentChangedEvent::*;
			match ev {
				Add    => self.changes.borrow_mut().push(ListChange::Added   { idx: ea.offs() }),
				Remove => self.changes.borrow_mut().push(ListChange::Removed { idx: ea.offs() }),
				Change => self.changes.borrow_mut().push(ListChange::Changed { idx: ea.offs() }),
			}
		}
	}
}

impl<'a> IContent<'a, EA> for SegmentView {
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
	seg:     SegmentView,
	backend: Rc<Backend>,
}

impl CodePane {
	// TODO: generate unique ID instead (could have multiple code panes open at once)
	const LIST_ID: &str = "panes.code.list";

	fn new(id: SegId, backend: Rc<Backend>) -> Self {
		Self {
			seg: SegmentView::new(id, backend.clone()),
			backend,
		}
	}

	fn set_segment(&mut self, id: SegId) {
		self.seg = SegmentView::new(id, self.seg.backend.clone());
	}

	fn dispatch_event(&self, ea: EA, ev: SegmentChangedEvent) {
		self.seg.dispatch_event(ea, ev);
	}

	fn view(&self) -> PaneContent<'_, Message> {
		let list = sparse_list(
			&self.seg,
			|_, ea: EA| {
				container(Rich::with_spans(self.seg.render_span(ea).render())
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
			}).id(Self::LIST_ID);

		let ui = container(list)
		.width(Length::Fill)
		.height(Length::Fill)
		.padding(Padding::from([0, 10]))
		.style(move |_theme| {
			container::Style::default().background(color!(0x101010))
		});

		let mut all_segs = self.backend.get_all_segments();
		all_segs.sort_by_key(|data| data.segid);
		// SAFETY: self.seg could only have been made from a valid segment ID
		let this_seg = all_segs.iter().find(|data| data.segid == self.seg.id()).unwrap().clone();

		let seg_selector = pick_list(
			all_segs,
			Some(this_seg),
			|segdata| Message::SwitchSegment { id: segdata.segid });

		PaneContent::new(ui)
		.title_bar(
			PaneTitleBar::new(text("Code").size(20).font(Font::DEFAULT.bold()))
				.padding(10)
				.controls(pane_grid::Controls::new(seg_selector))
				.always_show_controls()
		)
	}
}

// ------------------------------------------------------------------------------------------------
// PaneKind
// ------------------------------------------------------------------------------------------------

enum PaneKind {
	Names(NamesPane),
	Code(CodePane),
}

impl PaneKind {
	fn new_names(backend: Rc<Backend>) -> Self {
		Self::Names(NamesPane::new(backend))
	}

	fn new_code(id: SegId, backend: Rc<Backend>) -> Self {
		Self::Code(CodePane::new(id, backend))
	}

	fn view(&self) -> PaneContent<'_, Message> {
		match self {
			PaneKind::Names(n) => n.view(),
			PaneKind::Code(c)  => c.view(),
		}
	}

	fn as_code(&self) -> &CodePane {
		match self {
			PaneKind::Code(c) => c,
			_ => panic!(),
		}
	}

	fn as_code_mut(&mut self) -> &mut CodePane {
		match self {
			PaneKind::Code(c) => c,
			_ => panic!(),
		}
	}
}

// ------------------------------------------------------------------------------------------------
// Image loading and backend creation
// ------------------------------------------------------------------------------------------------

fn open_image() -> Image {
	// first try command-line arguments
	let args = std::env::args().collect::<Vec<_>>();

	if args.len() == 2 {
		match Image::new_from_file(&args[1]) {
			Ok(image) => return image,
			Err(e) => {
				error!("Could not open {:?}: {}", args[1], e);
				std::process::exit(1);
			}
		}
	}

	// then use a file dialog
	loop {
		let path = DialogBuilder::file()
			.set_location("~/src/re/adi/tests/data")
			.open_single_file()
			.show()
			.unwrap();

		match path {
			Some(path) => {
				match Image::new_from_file(&path) {
					Ok(image) => return image,
					Err(e) => {
						error!("Could not open {:?}: {}", path, e);
					}
				}
			}
			None => std::process::exit(1),
		}
	};
}

fn create_backend() -> Rc<Backend> {
	Rc::new(loop {
		let image = open_image();
		info!("opened image {}", image.name());

		match Backend::on_new_thread(image) {
			Ok(backend) => break backend,
			Err(e) => error!("Could not analyze {}", e),
		}
	})
}

// ------------------------------------------------------------------------------------------------
// AdiFE
// ------------------------------------------------------------------------------------------------

struct AdiFE {
	backend: Rc<Backend>,
	panes: PaneState<PaneKind>,
	name_pane: Pane,
	code_pane: Pane,
}

impl AdiFE {
	fn init() -> Self {
		AdiFE::new(create_backend())
	}

	fn new(backend: Rc<Backend>) -> Self {
		let (mut panes, name_pane) = PaneState::new(PaneKind::new_names(backend.clone()));
		let (code_pane, split) = panes.split(
			PaneAxis::Vertical, name_pane, PaneKind::new_code(
				SegId(3), // TODO: temporary
				backend.clone())).unwrap();
		panes.resize(split, 0.2);

		Self { backend: backend.clone(), panes, name_pane, code_pane }
	}

	fn subscriptions(&self) -> Subscription<Message> {
		time::every(milliseconds(300)).map(|_| Message::CheckForEvents)
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
			Message::PaneResized(PaneResizeEvent { split, ratio }) => {
				self.panes.resize(split, ratio);
			}
			Message::OperandClicked { bb_ea, instn, opn } => {
				println!("TODO: clicked operand {} of instruction {} in BB {:?}",
					opn, instn, bb_ea);
			}
			Message::JumpTo { ea } => {
				let code_pane = self.code_pane_mut();

				if code_pane.seg.id() != ea.seg() {
					code_pane.set_segment(ea.seg());
				}

				return operation::scroll_to(CodePane::LIST_ID, AbsoluteOffset {
					y: Some(f32::from_bits(ea.offs() as u32)), // item index
					x: Some(80.0),                             // pixel offset from top
				});
			}
			Message::SwitchSegment { id } => {
				let code_pane = self.code_pane_mut();

				if code_pane.seg.id() != id {
					code_pane.set_segment(id);
				}

				return operation::scroll_to(CodePane::LIST_ID, AbsoluteOffset {
					y: Some(f32::from_bits(0u32)), // item index
					x: Some(0.0),                  // pixel offset from top
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
			Message::CheckForEvents => {
				for event in self.backend.pending_events() {
					use BackendEvent::*;

					match event {
						SegmentChanged { ea, ev } => {
							println!("segment changed {:?} {:?}", ea, ev);
							self.code_pane().dispatch_event(ea, ev);
						}

						AutoAnalysisStatus { running } => {
							if running {
								println!("TODO: auto-analysis started");
							} else {
								println!("TODO: auto-analysis ended");
							}
						}
					}
				}
			}
			Message::ForceAnalyze => {
				self.backend.analyze_queue();
			}
		}

		Task::none()
	}

	fn view(&self) -> Element<'_, Message> {
		column![
			// trying to extract this callback into its own method is an exercise in frustration.
			// just leave it here unless you want to have the Worst Types and Where Clauses Ever.
			pane_grid(&self.panes, |_pane, state, _is_maximized| {
				state.view()
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
				space().width(10),
				button("Analyze").on_press(Message::ForceAnalyze),
			]
		].into()
	}
}