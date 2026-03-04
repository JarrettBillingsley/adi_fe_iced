
## TODO

- now that I'm no longer using `Rich` for code...
	- currently using vanilla `Column` for lines of code and `Row` for the contents of (some) lines
	- but in order to support a **virtual cursor** I'd kinda need to have my own versions of those?
	- thinking like...
		- `CodeLine` is a single line of code, column-based
			- Basically that `LineKind` enum would become the kind of `CodeLine`
			- It also supports the **virtual cursor** by knowing which column it's on
		- `CodeBlock` is a list of `CodeLine`s
			- Supports the **virtual cursor** by knowing which line it's on
		- `CodeView` ties it all together
			- Has the `SparseList` of `CodeBlocks`
			- Supports the **virtual cursor** by knowing which `CodeBlock` it's in (even if it's offscreen)
	- The **virtual cursor:**
		- clicking enables and places it 
			- and hides mouse cursor?
			- and then moving mouse disables **virtual cursor**?
		- current line/column, arrow keys move it
		- some lines are "real" (e.g. instructions, unknown/data bytes); other are "fake" (e.g. comments on instructions)
			- tho the "fake" ones are usually attached to a real one with an EA
		- if you arrow off top/bottom of one span, it focuses the next one
			- and possibly scrolls the code view
- location history, back/forward buttons
- when autoanalysis starts, lock and stop dispatching events to code view
	- remember *if* any segment changed events came in for it
	- then when autoanalysis ends, if any did, just have it refresh
	- OR.... rather than blocking out the code listing while analysis happens, have a Futures-based response model for backend queries
- when span change event comes in, `SparseList` should:
	- try to keep current **virtual cursor** position at the same Y coord
	- if there's no **virtual cursor** position, try to keep topmost visible line at the same Y coord
	- both of these kind of imply a deeper connection between `SparseList` and its content...
		- which makes it less reusable for e.g. the name list
		- maybe there can be a `CodeView` which *uses* a `SparseList`?
		- and extensbility in `SparseList` to support these things?
- name list
	- show EA (`NameListItem` should have a `TextEA` field)
	- filtering and sorting
	- dynamic changes (listener on `adi::NameMap`)
	- use `SparseList`? but how would that interact with sorting, what's the "index"?
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
	- resizing vertically smaller works fine. but resizing vertically bigger, weird clunky behavior where the topmost element is scrolled down and then back up repeatedly... why?
	
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

- **T**ype lets you choose type for a variable or blob o' bytes or whatever
- **F**unction starts/splits offs a new function
	- **Shift+F** sets the end of a function
- **B**ase cycles bases like before
	- or maybe brings up a quick menu sorta thing where you hit a second letter to choose repr?
	- or maybe **B** cycles bases and **R** brings up a "representation" quick menu?
- **S**tring
- **A**rray
- **E**num
