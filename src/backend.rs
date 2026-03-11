
use std::sync::{ Arc, OnceLock };
use std::sync::mpsc::{ Sender, Receiver, channel, TryIter as ChanTryIter };
use std::thread::{ Builder as ThreadBuilder };

use oneshot::{ Sender as OneshotSender, channel as oneshot_channel };

use adi::{ EA, VA, SegId, PlatformResult, Image, Program, Span, Type, SpanMapListener };

use crate::ui::{ CodeViewItem, SegmentData, NameListData };

// ------------------------------------------------------------------------------------------------
// modules
// ------------------------------------------------------------------------------------------------

mod macros;
mod render;
use macros::*;
use render::{ render_span };

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

/// Kinds of events the backend can notify the frontend of.
#[non_exhaustive]
#[derive(Copy, Clone)]
pub enum BackendEvent {
	/// Sent right after the backend is created, when the image has been loaded. `start_ea` is the
	/// address of the starting point of the program, so the UI can focus it.
	ImageLoaded { start_ea: EA },

	/// A segment's span map changed.
	SegmentChanged {
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
// Backend
// ------------------------------------------------------------------------------------------------

/// The frontend's "handle" to the backend. Abstracts away the fact that the backend is running on
/// another thread, and allows the frontend to interact with it as if it's just a normal object.
pub struct Backend {
	event_rx:   Receiver<BackendEvent>,
	command_tx: Sender<BackendCommand>,
}

impl Backend {
	/// Create a new backend on a new thread using the given [`Image`].
	pub fn on_new_thread(image: Image) -> PlatformResult<Self> {
		let (event_tx, event_rx) = channel();
		let (command_tx, command_rx) = channel();

		// I'd use a oneshot_channel here but when the thread exits, calling recv on the other
		// end of the channel will fail...
		let success: Arc<OnceLock<PlatformResult<()>>> = Arc::new(OnceLock::new());
		let success_clone = success.clone();

		ThreadBuilder::new().name("backend thread".into()).spawn(move || {
			let (prog, start_ea) = match adi::program_from_image(image) {
				Ok(tuple) => {
					success_clone.set(Ok(())).unwrap();
					tuple
				}
				Err(err) => {
					success_clone.set(Err(err)).unwrap();
					return; // bail!
				}
			};

			BackendThread { event_tx, command_rx }.main_loop(prog, start_ea);
		}).expect("failed to spawn backend thread!");

		success.wait().clone().map(|_| Self { event_rx, command_tx })
	}

	/// Interator over any pending events sent by the backend to the frontend. Looping over this
	/// iterator will exit the loop once there are no pending events left.
	pub fn pending_events(&self) -> ChanTryIter<'_, BackendEvent> {
		self.event_rx.try_iter()
	}

	// used by the methods declared by "invoke_with_tokens!(backend_command_methods..." below
	fn send_and_get<Ret>(&self, f: impl FnOnce(OneshotSender<Ret>) -> BackendCommand) -> Ret {
		let (tx, rx) = oneshot_channel();
		self.send(f(tx));
		rx.recv().expect("backend crashed??")
	}

	// used by the methods declared by "invoke_with_tokens!(backend_command_methods..." below
	fn send(&self, cmd: BackendCommand) {
		self.command_tx.send(cmd).expect("backend crashed??");
	}
}

// ------------------------------------------------------------------------------------------------
// SegmentListener
// ------------------------------------------------------------------------------------------------

/// Listens for changes on [`Segment`]s and sends them as [`BackendEvent`]s to the frontend.
struct SegmentListener {
	event_tx:   Sender<BackendEvent>,
	id:         SegId,
}

impl SegmentListener {
	fn new(event_tx: &Sender<BackendEvent>, id: SegId) -> Self {
		Self {
			event_tx: event_tx.clone(),
			id,
		}
	}

	fn event(&self, offs: usize, ev: SegmentChangedEvent) {
		self.event_tx.send(BackendEvent::SegmentChanged {
			ea: EA::new(self.id, offs),
			ev
		}).expect("UI thread crashed??");
	}
}

impl SpanMapListener for SegmentListener {
	fn span_added(&self, offs: usize) {
		self.event(offs, SegmentChangedEvent::Add);
	}
	fn span_removed(&self, offs: usize) {
		self.event(offs, SegmentChangedEvent::Remove);
	}
	fn span_changed(&self, offs: usize) {
		self.event(offs, SegmentChangedEvent::Change);
	}
}

// ------------------------------------------------------------------------------------------------
// BackendThread
// ------------------------------------------------------------------------------------------------

/// The actual backend thread which creates the [`Program`], listens for and responds to commands
/// from the frontend, and sends events when the `Program` changes.
struct BackendThread {
	event_tx:   Sender<BackendEvent>,
	command_rx: Receiver<BackendCommand>,
}

// used by `BackendThread::command_loop` which is defined by
// "invoke_with_tokens!(backend_thread_command_loop..." below
fn respond<T>(tx: OneshotSender<T>, response: T) {
	tx.send(response).expect("UI thread crashed??");
}

impl BackendThread {
	fn main_loop(self, mut prog: Program, start_ea: EA) {
		// TODO: temporary
		let state = prog.initial_mmu_state();
		prog.enqueue_new_func(state, prog.ea_from_name("VEC_RESET"));
		prog.enqueue_new_func(state, prog.ea_from_name("VEC_NMI"));
		let ty = Type::ptr(Type::Code, Type::U16);
		prog.new_data(Some("VEC_NMI_PTR"),   prog.ea_from_va(state, VA(0xFFFA)), ty.clone(), 2);
		prog.new_data(Some("VEC_RESET_PTR"), prog.ea_from_va(state, VA(0xFFFC)), ty.clone(), 2);
		prog.new_data(Some("VEC_IRQ_PTR"),   prog.ea_from_va(state, VA(0xFFFE)), ty.clone(), 2);
		prog.analyze_queue();

		// set these up *after* the initial analysis so as not to flood the UI with events.
		// (this is to avoid having a simultaneous mutable and immutable borrow on prog)
		for id in prog.all_segs().collect::<Vec<_>>().into_iter() {
			let listener = Box::new(SegmentListener::new(&self.event_tx, id));
			prog.segment_from_id_mut(id).attach_listener(Some(listener));
		}

		// let the frontend know!
		self.event(BackendEvent::ImageLoaded { start_ea });

		// this method is defined by "invoke_with_tokens!(backend_thread_command_loop..." below
		self.command_loop(prog);
	}

	fn event(&self, ev: BackendEvent) {
		self.event_tx.send(ev).expect("UI thread crashed??");
	}
}

// ------------------------------------------------------------------------------------------------
// Backend commands
// ------------------------------------------------------------------------------------------------

// These all become methods of `Backend`, but their code is run in `BackendThread::command_loop`.
export_backend_commands! {
	as backend_command_tokens,

	[self prog]

	// `self` is a bit of an oddity in macros. It's annoying to parse. To make that easier, I just
	// required it to be treated like any other argument, so it'll take the form `self: Type`

	/// Get all names. The returned vector is a tuple of each name's [`EA`] and the name itself.
	pub fn get_all_names(self: &Self) -> Vec<NameListData> {
		prog.all_names_by_ea()
			.map(|(ea, name)| NameListData { name: name.name.into_owned(), ea: ea })
			.collect::<Vec<_>>()
	}

	/// Get information about all segments.
	pub fn get_all_segments(self: &Self) -> Vec<SegmentData> {
		prog.all_segs()
		.map(|segid| {
			let seg = prog.segment_from_id(segid);
			SegmentData {
				segid,
				name:     seg.name().into(),
				is_image: seg.image().is_some(),
			}
		})
		.collect()
	}

	/// Gets the number of spans in `seg`.
	pub fn get_num_spans(self: &Self, seg: SegId) -> usize {
		prog.segment_from_id(seg).num_spans()
	}

	/// Renders a span at the given EA into a [`CodeViewItem`].
	pub fn get_rendered_span(self: &Self, ea: EA) -> CodeViewItem {
		render_span(&prog, ea)
	}

	/// Get the offset of the last span in `seg`.
	pub fn get_last_span_offset(self: &Self, seg: SegId) -> usize {
		prog.segment_from_id(seg).last_span_offset()
	}

	/// Get a span at the given EA.
	pub fn get_span(self: &Self, ea: EA) -> Span {
		prog.segment_from_id(ea.seg()).span_at_ea(ea)
	}

	/// Get a span before the given EA.
	pub fn get_span_before(self: &Self, ea: EA) -> Option<Span> {
		prog.segment_from_id(ea.seg()).span_before_ea(ea)
	}

	/// Get a span after the given EA.
	pub fn get_span_after(self: &Self, ea: EA) -> Option<Span> {
		prog.segment_from_id(ea.seg()).span_after_ea(ea)
	}

	/// Analyze any pending items in the analysis queue. This may take a while, and will generate
	/// [`BackendEvent`]s while it runs.
	pub fn analyze_queue(self: &Self) {
		self.event(BackendEvent::AutoAnalysisStatus { running: true });
		prog.analyze_queue();
		self.event(BackendEvent::AutoAnalysisStatus { running: false });
	}
}

invoke_with_tokens!(backend_command_enum, backend_command_tokens);
invoke_with_tokens!(backend_command_methods, backend_command_tokens);
invoke_with_tokens!(backend_thread_command_loop, backend_command_tokens);