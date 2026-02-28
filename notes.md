
- programmatic scrolling scroll_by - probably useful for arrow key navigation
- ditch Offset::Relative if not needed

- JANK:
	- short view, scroll to bottom, expand height -> items stay at top of view, until you scroll again. at least it doesn't crash
	- if you contract a visible BB, it pulls items up at first... but doesn't spawn new items after, and starts pulling the top of the BB down instead. why?