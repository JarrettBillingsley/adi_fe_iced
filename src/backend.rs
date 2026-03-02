#![allow(unused)]

use std::fmt::{ Write as FmtWrite };
use std::sync::{ Arc, OnceLock };
use std::sync::mpsc::{ Sender, Receiver, channel, TryIter as ChanTryIter };
use std::thread::{ Builder as ThreadBuilder };

use log::*;
use oneshot::{ Sender as OneshotSender, channel as oneshot_channel };

use adi::{ EA, VA, SegId, PlatformResult, Image, Program, Span, SpanKind, ImageSliceable,
	BasicBlock, DataItem, IPrintOutput, PrintStyle, FmtResult, Type };

use crate::ui::{ TextEA, CodeViewItem, BasicBlockData, CodeLineData,
	CodeOpData, UnknownData, UnknownLineData, SegmentData, FunctionData };

// ------------------------------------------------------------------------------------------------
// BackendEvent
// ------------------------------------------------------------------------------------------------

/// Kinds of changes that occur on a segment's span map.
#[non_exhaustive]
#[derive(Debug, Copy, Clone)]
pub enum SegmentChangedEvent {
	/// This is a new span.
	Add,
	/// The span was deleted.
	Remove,
	/// An existing span was changed in some way.
	Change,
}

// TO ADD A NEW BackendEvent:
// - enum variant
// - method in BackendThread
/// Kinds of events the backend can notify the frontend of.
#[non_exhaustive]
#[derive(Copy, Clone)]
pub enum BackendEvent {
	/// A segment's span map changed.
	SegmentChanged {
		// TODO: is this EA needed? well the segment ID is, at least.
		/// The EA of the span which changed.
		ea: EA,
		/// What kind of change it was.
		ev: SegmentChangedEvent,
	},

	/// Automatic analysis started (when `true`) or ended (when `false`). It won't respond to
	/// commands until the automatic analysis ends.
	AutoAnalysisStatus { running: bool },
}

// ------------------------------------------------------------------------------------------------
// BackendCommand
// ------------------------------------------------------------------------------------------------

// TO ADD A NEW BackendCommand:
// - new variant (with a tx: OneshotSender<T> if it has a response)
// - new method in Backend
// - new match arm in BackendThread::main_loop
/// Things the frontend can tell the backend to do.
///
/// `Get___` commands will respond instantly.
///
/// Other commands may take a while, and may generate [`BackendEvent`]s.
#[non_exhaustive]
#[derive(Debug)]
pub enum BackendCommand {
	/// Get all names. Responds with a `Vec<(EA, String)>`.
	GetAllNames { tx: OneshotSender<Vec<(EA, String)>> },

	/// Get all segments. Responds with a `Vec<SegmentData>`.
	GetAllSegments { tx: OneshotSender<Vec<SegmentData>> },

	/// Gets the number of spans in `seg`. Responds with a `usize`.
	GetNumSpans { seg: SegId, tx: OneshotSender<usize> },

	/// Renders a span at the given EA into a form displayable in the UI.
	GetRenderedSpan { ea: EA, tx: OneshotSender<CodeViewItem> },

	/// Get the offset of the last span in `seg`.
	GetLastSpanOffset { seg: SegId, tx: OneshotSender<usize> },

	/// Get a span at the given EA.
	GetSpan { ea: EA, tx: OneshotSender<Span> },

	/// Get a span before the given EA.
	GetSpanBefore { ea: EA, tx: OneshotSender<Option<Span>> },

	/// Get a span after the given EA.
	GetSpanAfter { ea: EA, tx: OneshotSender<Option<Span>> },
}

// ------------------------------------------------------------------------------------------------
// Backend
// ------------------------------------------------------------------------------------------------

pub struct Backend {
	event_rx:   Receiver<BackendEvent>,
	command_tx: Sender<BackendCommand>,
}

impl Backend {
	pub fn on_new_thread(image: Image) -> PlatformResult<Self> {
		let (event_tx, event_rx) = channel();
		let (command_tx, command_rx) = channel();

		// I'd use a oneshot_channel here but when the thread exits, calling recv on the other
		// end of the channel will fail...
		let success: Arc<OnceLock<PlatformResult<()>>> = Arc::new(OnceLock::new());
		let success_clone = success.clone();

		ThreadBuilder::new().name("backend thread".into()).spawn(move || {
			let mut prog = match adi::program_from_image(image) {
				Ok(prog) => {
					success_clone.set(Ok(())).unwrap();
					prog
				}
				Err(err) => {
					success_clone.set(Err(err)).unwrap();
					return; // bail!
				}
			};

			BackendThread { event_tx, command_rx }.main_loop(prog);
		}).expect("failed to spawn backend thread!");

		success.wait().clone().map(|_| Self { event_rx, command_tx })
	}

	pub fn get_all_names(&self) -> Vec<(EA, String)> {
		self.send_and_get(|tx| BackendCommand::GetAllNames { tx })
	}

	pub fn get_all_segments(&self) -> Vec<SegmentData> {
		self.send_and_get(|tx| BackendCommand::GetAllSegments { tx })
	}

	pub fn get_num_spans(&self, seg: SegId) -> usize {
		self.send_and_get(|tx| BackendCommand::GetNumSpans { seg, tx })
	}

	pub fn get_rendered_span(&self, ea: EA) -> CodeViewItem {
		self.send_and_get(|tx| BackendCommand::GetRenderedSpan { ea, tx })
	}

	pub fn get_last_span_offset(&self, seg: SegId) -> usize {
		self.send_and_get(|tx| BackendCommand::GetLastSpanOffset { seg, tx })
	}

	pub fn get_span(&self, ea: EA) -> Span {
		self.send_and_get(|tx| BackendCommand::GetSpan { ea, tx })
	}

	pub fn get_span_before(&self, ea: EA) -> Option<Span> {
		self.send_and_get(|tx| BackendCommand::GetSpanBefore { ea, tx })
	}

	pub fn get_span_after(&self, ea: EA) -> Option<Span> {
		self.send_and_get(|tx| BackendCommand::GetSpanAfter { ea, tx })
	}

	pub fn pending_events(&self) -> ChanTryIter<'_, BackendEvent> {
		self.event_rx.try_iter()
	}

	fn send_and_get<Ret>(&self, f: impl FnOnce(OneshotSender<Ret>) -> BackendCommand) -> Ret {
		let (tx, rx) = oneshot_channel();
		self.send(f(tx));
		rx.recv().expect("backend crashed??")
	}

	fn send(&self, cmd: BackendCommand) {
		self.command_tx.send(cmd).expect("backend crashed??");
	}
}

// ------------------------------------------------------------------------------------------------
// BackendThread
// ------------------------------------------------------------------------------------------------

struct BackendThread {
	event_tx:   Sender<BackendEvent>,
	command_rx: Receiver<BackendCommand>,
}

impl BackendThread {
	fn main_loop(self, mut prog: Program) {

		let state = prog.initial_mmu_state();
		prog.enqueue_new_func(state, prog.ea_from_name("VEC_RESET"));
		prog.enqueue_new_func(state, prog.ea_from_name("VEC_NMI"));
		let ty = Type::ptr(Type::Code, Type::U16);
		prog.new_data(Some("VEC_NMI_PTR"),   prog.ea_from_va(state, VA(0xFFFA)), ty.clone(), 2);
		prog.new_data(Some("VEC_RESET_PTR"), prog.ea_from_va(state, VA(0xFFFC)), ty.clone(), 2);
		prog.new_data(Some("VEC_IRQ_PTR"),   prog.ea_from_va(state, VA(0xFFFE)), ty.clone(), 2);
		prog.analyze_queue();

		for command in self.command_rx.iter() {
			use BackendCommand::*;
			match command {
				GetAllNames { tx } => {
					trace!("GetAllNames");

					let response = prog.all_names_by_ea()
						.map(|(ea, name)| (*ea, name.clone()))
						.collect();
					respond(tx, response);
				}
				GetAllSegments { tx } => {
					trace!("GetAllSegments");
					let response = prog.all_segs()
						.map(|segid| {
							let seg = prog.segment_from_id(segid);
							SegmentData {
								segid,
								name:     seg.name().into(),
								is_image: seg.image().is_some(),
							}
						})
						.collect();
					respond(tx, response);
				}
				GetNumSpans { seg, tx } => {
					let response = prog.segment_from_id(seg).num_spans();
					trace!("GetNumSpans {:?} => {}", seg, response);
					respond(tx, response);
				}
				GetRenderedSpan { ea, tx } => {
					trace!("GetRenderedSpan {:?}", ea);
					let response = self.render_span(&prog, ea);
					respond(tx, response);
				}
				GetLastSpanOffset { seg, tx } => {
					trace!("GetLastSpanOffset {:?}", seg);
					let response = prog.segment_from_id(seg).last_span_offset();
					respond(tx, response);
				}
				GetSpan { ea, tx } => {
					trace!("GetSpanSpan {:?}", ea);
					let response = prog.segment_from_id(ea.seg()).span_at_ea(ea);
					respond(tx, response);
				}
				GetSpanBefore { ea, tx } => {
					trace!("GetSpanSpan {:?}", ea);
					let response = prog.segment_from_id(ea.seg()).span_before_ea(ea);
					respond(tx, response);
				}
				GetSpanAfter { ea, tx } => {
					trace!("GetSpanSpan {:?}", ea);
					let response = prog.segment_from_id(ea.seg()).span_after_ea(ea);
					respond(tx, response);
				}
			}
		}
	}

	fn auto_analysis_status_event(&self, running: bool) {
		self.event(BackendEvent::AutoAnalysisStatus { running });
	}

	fn segment_change_event(&self, ea: EA) {
		self.event(BackendEvent::SegmentChanged { ea, ev: SegmentChangedEvent::Change });
	}

	fn segment_add_event(&self, ea: EA) {
		self.event(BackendEvent::SegmentChanged { ea, ev: SegmentChangedEvent::Add });
	}

	fn segment_remove_event(&self, ea: EA) {
		self.event(BackendEvent::SegmentChanged { ea, ev: SegmentChangedEvent::Remove });
	}

	fn event(&self, ev: BackendEvent) {
		self.event_tx.send(ev).expect("UI thread crashed??");
	}

	fn render_span(&self, prog: &Program, ea: EA) -> CodeViewItem {
		let span = prog.span_at_ea(ea);

		match span.kind() {
			SpanKind::Unk      => self.render_unk(prog, &span),
			SpanKind::Code(id) => self.render_bb(prog, prog.get_bb(id)),
			SpanKind::Data(id) => self.render_data(prog, prog.get_data(id)),
			_ => panic!("uhhhhh why are we trying to render an in-progress span?"),
		}
	}

	fn bb_func_differs_from_previous(&self, prog: &Program, bb: &BasicBlock) -> bool {
		let seg = prog.segment_from_ea(bb.ea());
		if let Some(span) = seg.span_before_ea(bb.ea()) {
			if let Some(func) = prog.func_that_contains(span.start()) {
				if func.id() != bb.func() {
					return true;
				}
			}
		}

		false
	}

	// if this bb's function differs from the function (if any) that owns the previous span, we need
	// to show either a function header or a function piece header.
	fn render_bb_header(&self, prog: &Program, bb: &BasicBlock) -> FunctionData {
		let func = prog.get_func(bb.func());

		if self.bb_func_differs_from_previous(prog, bb) {
			let name = prog.name_of_ea(func.ea());

			if bb.id() == func.head_id() {
				let attrs = if !func.attrs().is_empty() {
					format!("{:?}", func.attrs())
				} else {
					"".to_string()
				};
				let entrypoints = if func.is_multi_entry() {
					func.entrypoints().iter()
						.map(|bbid| prog.name_of_ea(prog.get_bb(*bbid).ea()))
						.collect::<Vec<_>>()
						.join(", ")
				} else {
					"".to_string()
				};

				FunctionData {
					name: name.into(),
					is_piece: false,
					attrs: attrs.into(),
					entrypoints: entrypoints.into(),
				}
			} else {
				FunctionData {
					name: name.into(),
					is_piece: true,
					..Default::default()
				}
			}
		} else {
			Default::default()
		}
	}

	fn render_bb_code(&self, prog: &Program, bb: &BasicBlock) -> Vec<CodeLineData> {
		let mut ret = vec![];

		let seg = prog.segment_from_ea(bb.ea());
		let seg_name = seg.name();
		let state = bb.mmu_state();

		for inst in bb.insts() {
			let mut bytes = String::new();
			let b = inst.bytes();

			match b.len() {
				1 => write!(bytes, "{:02X}",               b[0]).unwrap(),
				2 => write!(bytes, "{:02X} {:02X}",        b[0], b[1]).unwrap(),
				3 => write!(bytes, "{:02X} {:02X} {:02X}", b[0], b[1], b[2]).unwrap(),
				_ => unreachable!()
			}

			let mut output = UIRenderOutput::new();
			prog.inst_print(inst, state, &mut output).unwrap();
			let (mnemonic, operands) = output.finish();

			ret.push(CodeLineData {
				ea:    TextEA::new(seg_name, &prog.fmt_addr(inst.va().0)),
				bytes: bytes.into(),
				mnemonic: mnemonic.into(),
				operands,
			});
		}

		ret
	}

	fn render_bb(&self, prog: &Program, bb: &BasicBlock) -> CodeViewItem {
		let func_header = self.render_bb_header(prog, bb);
		let label = if prog.get_inrefs(bb.ea()).is_some() {
			prog.name_of_ea(bb.ea())
		} else {
			"".to_string()
		};

		CodeViewItem::BasicBlock(BasicBlockData {
			ea:    bb.ea().into(),
			func:  func_header,
			label: label.into(),
			lines: self.render_bb_code(prog, bb),
		})
	}

	fn render_data(&self, prog: &Program, bb: &DataItem) -> CodeViewItem {
		Default::default()
	}

	fn render_unk(&self, prog: &Program, span: &Span) -> CodeViewItem {
		// TODO: these should be configurable
		const UNK_SIZE_CUTOFF: usize = 128;
		const UNK_STRIDE: usize = 16;

		let ea       = span.start();
		let seg      = prog.segment_from_ea(ea);
		let state    = prog.mmu_state_at(ea).unwrap_or_else(|| prog.initial_mmu_state());
		let va       = prog.va_from_ea(state, ea);
		let seg_name = seg.name();

		let mut lines = vec![UnknownLineData {
			ea:    TextEA::new(seg_name, &prog.fmt_addr(va.0)),
			bytes: format!("[{} unexplored byte(s)]", span.len()).into()
		}];

		if seg.is_real() {
			let len = span.len().min(UNK_SIZE_CUTOFF);
			let slice = seg.image_slice(ea .. ea + len);
			let data = slice.data();
			let mut addr = prog.fmt_addr(va.0);

			for (i, chunk) in data.chunks(UNK_STRIDE).enumerate() {
				let mut bytes = String::with_capacity(chunk.len() * 3);

				bytes.push_str(&format!("{:02X}", chunk[0]));

				for byte in &chunk[1 ..] {
					bytes.push_str(&format!(" {:02X}", byte));
				}

				addr = prog.fmt_addr(va.0 + i * UNK_STRIDE);
				lines.push(UnknownLineData {
					ea: TextEA::new(seg_name, &addr),
					bytes: bytes.into(),
				});
			}

			if span.len() > UNK_SIZE_CUTOFF {
				lines.push(UnknownLineData {
					ea: TextEA::new(seg_name, &addr),
					bytes: "...".into(),
				});
			}
		}

		CodeViewItem::Unknown(UnknownData { lines })
	}
}

fn respond<T>(tx: OneshotSender<T>, response: T) {
	tx.send(response).expect("UI thread crashed??");
}

// ------------------------------------------------------------------------------------------------
// UIRenderOutput
// ------------------------------------------------------------------------------------------------

struct UIRenderOutput {
	mnemonic:   Option<String>,
	operands:   Vec<CodeOpData>,
	tmp_str:    String,
	tmp_style:  Option<PrintStyle>,
	tmp_opn:    Option<u8>,
}

impl UIRenderOutput {
	fn new() -> Self {
		Self {
			mnemonic:   None,
			operands:   vec![],
			tmp_str:    String::new(),
			tmp_style:  None,
			tmp_opn:    None,
		}
	}

	fn finish(mut self) -> (String, Vec<CodeOpData>) {
		// if there's anything still hanging around in the buffer, output it as plain text
		if !self.tmp_str.is_empty() {
			self.operands.push(
				CodeOpData::new_plain(std::mem::take(&mut self.tmp_str)));
		}

		(self.mnemonic.unwrap_or("???".to_string()), self.operands)
	}
}

impl FmtWrite for UIRenderOutput {
	fn write_str(&mut self, s: &str) -> FmtResult {
		self.tmp_str.write_str(s)
	}
}

impl IPrintOutput for UIRenderOutput {
	fn begin(&mut self, style: PrintStyle) -> FmtResult {
		// if something is in the buffer, it was printed *outside* of any begin/end calls; so output
		// it as plain text.
		if !self.tmp_str.is_empty() {
			self.operands.push(CodeOpData::new_plain(std::mem::take(&mut self.tmp_str)));
		}

		use PrintStyle::*;
		match style {
			Mnemonic => {
				assert!(self.mnemonic.is_none());
			}

			Register | Number | Symbol | String | Comment | Refname | Label => {
				self.tmp_style = Some(style);
			}

			Operand(opn) => {
				self.tmp_opn = Some(opn as u8);
			}

			_ => todo!("a new PrintStyle was added!"),
		}

		Ok(())
	}

	fn end(&mut self, style: PrintStyle) -> FmtResult {
		use PrintStyle::*;
		match style {
			Mnemonic => {
				self.mnemonic = Some(std::mem::take(&mut self.tmp_str));
			}

			Register | Number | Symbol | String | Comment | Refname | Label => {
				self.operands.push(CodeOpData::new(std::mem::take(&mut self.tmp_str),
					self.tmp_style,
					self.tmp_opn)); // works regardless of if we're in an operand
			}

			Operand(opn) => {
				self.tmp_opn = None;

				if !self.tmp_str.is_empty() {
					self.operands.push(CodeOpData::new(std::mem::take(&mut self.tmp_str),
						self.tmp_style,
						Some(opn as u8)));
				}
			}

			_ => todo!("a new PrintStyle was added!"),
		}
		Ok(())
	}
}
