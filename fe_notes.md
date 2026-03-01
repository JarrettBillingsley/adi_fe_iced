
- separate text::Rich for each line of text?
- make code lines more structured - column-based
	- location (seg:offs)
	- code bytes
	- label
	- instruction/data/unknown
	- outrefs
- first window should have a list of recent files
	- *then* if you want to open a new one, it opens the file dialog
	- really that whole initial process is just a temporary thing

---

## `SparseList`

- JANK:
	- short view, scroll to bottom, expand height -> items stay at top of view, until you scroll again. at least it doesn't crash
	- if you contract a visible BB, it pulls items up at first... but doesn't spawn new items after, and starts pulling the top of the BB down instead. why?