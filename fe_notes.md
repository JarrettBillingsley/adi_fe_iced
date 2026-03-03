
- when autoanalysis starts, lock and stop dispatching events to code view
	- remember *if* any segment changed events came in for it
	- then when autoanalysis ends, if any did, just have it refresh
- custom widget for displaying code instead of `text::Rich`
	- don't need most of its complexity
	- need to listen for more events than just clicking links
	- also makes location history easier (esp. if `SparseList` can know e.g. what the topmost line on the display is)
	- also makes it easier to have variable-width columns
	- this thing would support a **virtual text cursor**
		- clicking enables and places it 
			- and hides mouse cursor?
			- and then moving mouse disables virtual text cursor?
		- current line/column, arrow keys move it
		- some lines are "real" (e.g. instructions, unknown/data bytes); other are "fake" (e.g. comments on instructions)
			- tho the "fake" ones are usually attached to a real one with an EA
		- if you arrow off top/bottom of one span, it focuses the next one
			- and possibly scrolls the code view
- make code lines more structured - column-based
	- location (seg:offs)
	- code bytes
	- label
	- instruction/data/unknown
	- outrefs
- location history, back/forward buttons
- when span change event comes in, `SparseList` should:
	- try to keep current virtual cursor position at the same Y coord
	- if there's no virtual cursor position, try to keep topmost visible line at the same Y coord
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

---

## `SparseList`

- JANK:
	- short view, scroll to bottom, expand height -> items stay at top of view, until you scroll again. at least it doesn't crash
	- if you contract a visible BB, it pulls items up at first... but doesn't spawn new items after, and starts pulling the top of the BB down instead. why?