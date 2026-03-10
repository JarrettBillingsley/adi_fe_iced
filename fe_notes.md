
## TODO

- now that I'm no longer using `Rich` for code...
	- `CodeLine` is a single line of code, replaces `Row`
		- a single `EA` in the original code can have multiple `CodeLine`s for things like comments, labels, inrefs...
	- `CodeBlock` is a list of `CodeLine`s, replaces `Column`, corresponds to an `adi::Span`
		- Supports the **text cursor** by knowing which line it's on?
		- Or maybe `CodeView` knows that? not sure
		- Maybe this type is not even needed? Idk, `Column` is kind of opaque, and we're gonna have to be adding children in places other than the end (`Column::push` only adds to the end)
	- `CodeView` ties it all together
		- Has the `SparseList` of `CodeBlock`s
		- Supports the **text cursor** by knowing which `CodeBlock` it's in (even if it's offscreen)
			- and also the line and column (so it can remember where it is offscreen)
		- Handles up/down arrow keys, since those can change the current line or block, and it also has the `SparseList` which needs to be notified to scroll that line into view
- location history, back/forward buttons
- when autoanalysis starts, lock and stop dispatching events to code view
	- remember *if* any segment changed events came in for it
	- then when autoanalysis ends, if any did, just have it refresh
	- OR.... rather than blocking out the code listing while analysis happens, have a Futures-based response model for backend queries
- when span change event comes in, `CodeView` should:
	- try to keep current **text cursor** position at the same Y coord in the `SparseList`
	- if there's no **text cursor** position, try to keep topmost visible line at the same Y coord
	- need extensbility in `SparseList` to support these things?
- name list
	- show EA (`NameListItem` should have a `TextEA` field) 
		- use a `Table` for it?
		- or use `SparseList`? but how would that interact with sorting, what's the "index"?
	- filtering and sorting
	- dynamic changes (listener on `adi::NameMap`)
- first window should have a list of recent files
	- *then* if you want to open a new one, it opens the file dialog
	- really that whole initial process is just a temporary thing
- data rendering!
- how do we handle e.g. if they rename one of the entry points, knowing to update the entrypoints member of this data?

---

## `SparseList`

- JANK:
	- short view, scroll to bottom, expand height -> items stay at top of view, until you scroll again. at least it doesn't crash
	- if you contract a visible BB, it pulls items up at first... but doesn't spawn new items after, and starts pulling the top of the BB down instead. why?

---

## IDA Keybindings and my thoughts on them

- S tier
	- **Enter** - navigate to (obvious)
	- **Esc** - back          (even faster than some cmd+left or whatever)
	- **G**oto                (no notes)
	- **X**ref                (beautiful)
	- re**N**ame              (though I like Ghidra's **L**abel for this too)
	- **C**ode                (perfect)
	- **D**ata                (yep)
	- **U**ndefine            (mhm)
	- **O**ffset              (yep)
- works but not married to it
		- t**Y**pe     (arguably T would work better, doing that more often than selecting struct field)
		- create **P**rocedure (who calls them procedures?)
		- **E**nd procedure    (s'fine, but what about Shift+whatever creates a function? relate them)
		- **B**inary           (IIRC this actually cycles bases, which is good)
		- array (**\*** like in `int*` I guess?) (I like **A** or **[** better for this)
	- **#** (number, default) - how often is this needed?
	- bitwise NOT (**~**)
- dumb
		- string (**A**scii? weird)
	- hex (**Q**??????)
	- cha**R**acter (I know **C** is taken but...)
		- **S**egment (what a waste!!!! use this for string instead)
	- s**T**ruct field (uhhhh how about **F** for field lol... unless function uses that, shit)
		- enu**M** (**E**? if **E** is freed up by using something else to end a function)
	- change sign (**\_**) (why not just **-**???)

so what if...

- **Enter** - navigate to
- **Esc** - back
- **G**oto
- **X**ref
- re**N**ame
- **C**ode
- **D**ata
- **U**ndefine
- **O**ffset
- **T**ype lets you choose type for a variable or blob o' bytes or whatever
- **F**unction starts/splits offs a new function
	- **Shift+F** sets the end of a function
- **B**ase cycles bases like before
	- or maybe brings up a quick menu sorta thing where you hit a second letter to choose repr?
	- or maybe **B** cycles bases and **R** brings up a "representation" quick menu?
- **S**tring
- **A**rray
- **E**num
