
use std::rc::{ Rc };

use iced::{
	Element, Font, Task, Subscription,
	font::{ Weight },
	time::{ self, milliseconds },
	widget::{
		column, row, button, space,

		pane_grid,
		pane_grid::{
			Pane,
			Axis as PaneAxis,
			Content as PaneContent,
			// DragEvent as PaneDragEvent,
			ResizeEvent as PaneResizeEvent,
			State as PaneState,
		},

		operation::{ self, AbsoluteOffset, RelativeOffset },
	},
};

use adi::{ SegId, Image };

use simplelog::{ *, Color as SimpleLogColor };
use log::*;
use better_panic::{ Settings as PanicSettings, Verbosity as PanicVerbosity };
use native_dialog::{ DialogBuilder };

// ------------------------------------------------------------------------------------------------
// Modules
// ------------------------------------------------------------------------------------------------

mod backend;
mod ui;
mod widgets {
	pub mod code_line;
	pub mod code_pane;
	pub mod code_view;
	pub mod name_pane;
	pub mod sparse_list;
}

use backend::{ Backend, BackendEvent };
use widgets::code_view::{ CodeViewMessage, OperandLocation };
use widgets::code_pane::{ CodePane };
use widgets::name_pane::{ NamePane };

// ------------------------------------------------------------------------------------------------
// main
// ------------------------------------------------------------------------------------------------

fn main() -> Result<(), Box<dyn std::error::Error>> {
	setup_logging(LevelFilter::Trace)?;
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
pub(crate) const CONSOLAS_FONT: Font = Font::with_name("Consolas");
pub(crate) const CONSOLAS_FONT_BOLD: Font = Font { weight: Weight::Bold, ..CONSOLAS_FONT };

pub(crate) trait FontEx {
	fn bold(&self) -> Font;
}

impl FontEx for Font {
	fn bold(&self) -> Font {
		Font { weight: Weight::Bold, ..*self }
	}
}

// ------------------------------------------------------------------------------------------------
// Message
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub(crate) enum Message {
	// PaneDragged(PaneDragEvent),
	PaneResized(PaneResizeEvent),
	CodeView(CodeViewMessage),
	CheckForEvents,
	ForceAnalyze,
}

impl From<CodeViewMessage> for Message {
	fn from(cvm: CodeViewMessage) -> Self {
		Self::CodeView(cvm)
	}
}

// ------------------------------------------------------------------------------------------------
// PaneKind
// ------------------------------------------------------------------------------------------------

enum PaneKind {
	Name(NamePane),
	Code(CodePane),
}

impl PaneKind {
	fn new_name(backend: Rc<Backend>) -> Self {
		Self::Name(NamePane::new(backend))
	}

	fn new_code(id: SegId, backend: Rc<Backend>) -> Self {
		Self::Code(CodePane::new(id, backend))
	}

	fn view(&self) -> PaneContent<'_, Message> {
		match self {
			PaneKind::Name(n) => n.view(),
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
	#[allow(dead_code)] // TODO: temporary
	name_pane: Pane,
	code_pane: Pane,
	cur_operand: Option<OperandLocation>
}

impl AdiFE {
	fn init() -> Self {
		AdiFE::new(create_backend())
	}

	fn new(backend: Rc<Backend>) -> Self {
		let (mut panes, name_pane) = PaneState::new(PaneKind::new_name(backend.clone()));
		let (code_pane, split) = panes.split(
			PaneAxis::Vertical, name_pane, PaneKind::new_code(
				SegId(3), // TODO: temporary
				backend.clone())).unwrap();
		panes.resize(split, 0.2);

		Self {
			backend: backend.clone(),
			panes,
			name_pane,
			code_pane,
			cur_operand: None,
		}
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
		use Message::*;
		match message {
			// PaneDragged(de) => {
			// 	println!("TODO: dragged {:?}", de);
			// }
			PaneResized(PaneResizeEvent { split, ratio }) => {
				self.panes.resize(split, ratio);
				Task::none()
			}
			CodeView(cvm) => self.handle_code_view_message(cvm),
			CheckForEvents => self.check_for_events(),
			ForceAnalyze => {
				self.backend.analyze_queue();
				Task::none()
			}
		}
	}

	fn handle_code_view_message(&mut self, cvm: CodeViewMessage) -> Task<Message> {
		use CodeViewMessage::*;
		match cvm {
			OperandHovered { loc, over } => {
				// TODO: this and OperandFocused need to interact in a more subtle way
				if over {
					self.cur_operand = Some(loc);
					println!("TODO: hovered over BB {:?} instruction #{} operand #{}",
						loc.bb_ea, loc.instn, loc.opn);
				} else if let Some(cur_operand) = self.cur_operand && cur_operand == loc {
					self.cur_operand = None;
					println!("TODO: hovering over nothing");
				}
			}
			OperandClicked { loc, double } => {
				println!("TODO: {}-clicked BB {:?} instruction #{} operand #{}",
					if double { "double" } else { "single" },
					loc.bb_ea, loc.instn, loc.opn);
			}
			OperandFocused { loc, over } => {
				if over {
					self.cur_operand = Some(loc);
					println!("TODO: text cursor moved over BB {:?} instruction #{} operand #{}",
						loc.bb_ea, loc.instn, loc.opn);
				} else if let Some(cur_operand) = self.cur_operand && cur_operand == loc {
					self.cur_operand = None;
					println!("TODO: text cursor over nothing");
				}
			}
			JumpTo { ea } => {
				self.code_pane_mut().set_segment(ea.seg());

				return operation::scroll_to(CodePane::CODEVIEW_ID, AbsoluteOffset {
					y: Some(f32::from_bits(ea.offs() as u32)), // item index
					x: Some(80.0),                             // pixel offset from top
				});
			}
			SwitchSegment { id } => {
				self.code_pane_mut().set_segment(id);

				// important to scroll here or else the code view will get Very Mad and Crash
				return operation::scroll_to(CodePane::CODEVIEW_ID, AbsoluteOffset {
					y: Some(f32::from_bits(0u32)), // item index
					x: Some(0.0),                  // pixel offset from top
				});
			}
			JumpToTop =>  {
				return operation::snap_to(CodePane::CODEVIEW_ID, RelativeOffset {
					x: None,
					y: Some(0.0),
				});
			}
			JumpToBottom =>  {
				return operation::snap_to(CodePane::CODEVIEW_ID, RelativeOffset {
					x: None,
					y: Some(1.0),
				});
			}
			Scroll { up } => {
				return operation::scroll_by(CodePane::CODEVIEW_ID, AbsoluteOffset {
					x: 0.0,
					y: if up { -20.0 } else { 20.0 },
				});
			}
		}

		Task::none()
	}

	fn check_for_events(&self) -> Task<Message> {
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

		Task::none()
	}

	fn view(&self) -> Element<'_, Message> {
		// TODO: is the view *supposed* to be recreated every time we get a CheckForEvents message?
		// log::warn!("recreated entire window view");
		column![
			// trying to extract this callback into its own method is an exercise in frustration.
			// just leave it here unless you want to have the Worst Types and Where Clauses Ever.
			pane_grid(&self.panes, |_pane, state, _is_maximized| {
				state.view()
			})
			// .on_drag(Message::PaneDragged)
			.on_resize(10, Message::PaneResized)
			.min_size(200),

			row![
				button("top").on_press(CodeViewMessage::JumpToTop.into()),
				space().width(10),
				button("bottom").on_press(CodeViewMessage::JumpToBottom.into()),
				space().width(10),
				button("^").on_press(CodeViewMessage::Scroll { up: true }.into()),
				space().width(10),
				button("v").on_press(CodeViewMessage::Scroll { up: false }.into()),
				space().width(10),
				button("Analyze").on_press(Message::ForceAnalyze),
			]
		].into()
	}
}