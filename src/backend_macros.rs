//! Originally by T-Dark, Potassium Shill (@t.dark) on the Rust Programming Language Community
//! Discord, modified for my uses
//!
//! licensed CC0

// If it helps, the playground has an "expand macros" button under "Tools" in the top-right corner,
// which you can press to see what all of this expands to. To do this locally, you could instead
// run `cargo expand`

// So. Your first problem is that you need to generate three different things in at least two
// different places(and preferably three, for clarity) from one macro invocation. To avoid having
// to write three identical macro bodies, we do a setup inspired by the `macro-magic` crate to
// allow a macro invocation to import tokens defined elsewhere.
//
// The idea is conceptually simple: `export_backend_commands` lets you write the arguments that you
// want to pass to all three macros, and `invoke_with_tokens` invokes the macro you tell it to
// passing it the exported tokens.
//
// To do this, `export_backend_commands` defines a new macro (named whatever you want) whose body is
// just your tokens. and `invoke_with_tokens` just calls this macro, passing it the macro you're
// trying to invoke as an argument. Lastly, the generated macro calls its argument passing it all
// of the tokens. It's a little messy to understand the first time, but it's not that bad once you
// do.
//
// You could actually simplify away both `export_backend_commands` and `invoke_with_tokens` by doing
// what they do manually. You may prefer it, it's a bit simpler

macro_rules! export_backend_commands {
	(as $name:ident, $($tokens:tt)*) => {
		export_backend_commands! { @inner [$] as $name, $($tokens)* }
	};
	// Workaround for the fact I need to generate a macro definition that contains `$`s. To do this,
	// I take a `$` as a token(in the `d` metavariable) and paste it where needed. If I used
	// a _literal_ `$`, it would be taken to mean I want to expand a metavariable defined by
	// `export_backend_commands`, not by the generated macro
	(@inner [$d:tt] as $name:ident, $($tokens:tt)*) => {
		macro_rules! $name {
			($d ($d path:ident)::+) => {
				$d ($d path)::+ ! { $($tokens)* }
			}
		}
		// use $name;
	};
}
// macro scoping is very funky. It can be made a lot more normal if you `use` the macro after its
// definition.
pub(crate) use export_backend_commands;

macro_rules! invoke_with_tokens {
	($macro:ident, $($tokens_path:ident)::+) => {
		$($tokens_path)::+ ! { $macro }
	}
}
pub(crate) use invoke_with_tokens;

// ------------------------------------------------------------------------------------------------

macro_rules! backend_command_enum_tx {
	($type:ty) => { OneshotSender<$type> };
	() => { () };
}
pub(crate) use backend_command_enum_tx;

// Macro number 1. Its input is identical to what I passed to `export_backend_commands`, and it
// generates an enum for the messages and one for the replies.
macro_rules! backend_command_enum {
	([$global_self:ident $global_prog:ident] $(
		$vis:vis fn $fn_name:ident
			( $self:ident : $self_ty:ty $(, $arg_name:ident : $arg_type:ty)* )
			$(-> $ret_type:ty)? $body:block
	)*) => {
		#[non_exhaustive]
		#[derive(Debug)]
		#[allow(non_camel_case_types)]
		enum BackendCommand {
			$(
				$fn_name {
					tx: backend_command_enum_tx!($($ret_type)?),
					$($arg_name : $arg_type),*
				}
			),*
		}
	};
}
pub(crate) use backend_command_enum;

macro_rules! backend_command_method_body {
	( $fn_name:ident ($self:ident, $($arg_name:ident)*) -> $ret_type:ty) => {
		$self.send_and_get(|tx| BackendCommand::$fn_name {
			tx,
			$($arg_name),*
		})
	};

	( $fn_name:ident ($self:ident, $($arg_name:ident)*) -> ) => {
		$self.send(BackendCommand::$fn_name {
			tx: (),
			$($arg_name),*
		});
	};
}
pub(crate) use backend_command_method_body;

// Macro number two. This one generates all the functions on the backend handle
macro_rules! backend_command_methods {
	([$global_self:ident $global_prog:ident] $(
		$vis:vis fn $fn_name:ident
			( $self:ident : $self_ty:ty $(, $arg_name:ident : $arg_type:ty)* )
			$(-> $ret_type:ty)? $body:block
	)*) => {
		impl Backend {
			$(
				$vis fn $fn_name (&self, $( $arg_name : $arg_type ),* ) $(-> $ret_type)? {
					backend_command_method_body! { $fn_name ( self, $($arg_name)* ) -> $($ret_type)? }
				}
			)*
		}
	};
}
pub(crate) use backend_command_methods;

macro_rules! backend_thread_command_loop_arm_body {
	($tx:ident $self:ident $body:block -> $ret_type:ty) => {
		respond($tx, $body);
	};

	($tx:ident $self:ident $body:block -> ) => {
		let _ = $tx;
		$body;
	};
}
pub(crate) use backend_thread_command_loop_arm_body;

// Macro number three. This one pastes all the function definitions we've written earlier, and
// then writes a `handle_messages` function which receives messages from the receiver,
// dispatches to the appropriate function, and sends a reply.
macro_rules! backend_thread_command_loop {
	([$global_self:ident $global_prog:ident] $(
		$vis:vis fn $fn_name:ident
			( $self:ident : $self_ty:ty $(, $arg_name:ident : $arg_type:ty)* )
			$(-> $ret_type:ty)? $body:block
	)*) => {
		impl BackendThread {
			// $( $vis fn $fn_name ($self : $self_ty, $($arg_name : $arg_type),*) $(-> $ret_type)? $body )*
			fn command_loop($global_self, mut prog: Program) {
				for command in $global_self.command_rx.iter() {
					match command {$(
						BackendCommand::$fn_name { tx, $($arg_name),* } => {
							#[allow(unused_mut)]
							let mut $global_prog = &mut prog;
							backend_thread_command_loop_arm_body!(tx $global_self $body -> $($ret_type)?);
						}
					)*}
				}
			}
		}
	};
}
pub(crate) use backend_thread_command_loop;