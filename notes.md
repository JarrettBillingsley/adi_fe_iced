
- programmatic scrolling - is that what the operation thing is for?
- ditch Offset::Relative if not needed

- JANK:
	- short view, scroll to bottom, expand height -> items stay at top of view, until you scroll again. at least it doesn't crash
	- if you contract a visible BB, it pulls items up at first... but doesn't spawn new items after, and starts pulling the top of the BB down instead. why?

---

before we scroll, we ask, "would scrolling by this much put us in the danger zone?" so:

- when scrolling down:
	- if bottom-most manifested element's bottom moves **above new_view_bottom,**
		- append more items until bottom is >= new_view_bottom
	- if top-most manifested element's bottom moves **above new_offset_y,**
		- remove items at start until top-most bottom is below new_offset_y
		- each removal also needs to adjust the scroll position!
- when scrolling up:
	- if top-most element's top moves **below new_offset_y,**
		- prepend more items until top is <= new_offset_y
		- each prepend also needs to adjust the scroll position!
	- if bottom-most element's top moves **below new_view_bottom,**
		- remove items at end until bottom-most top is above new_view_bottom

the State::scroll() method already handles the clamping of the scroll at the content bounds, so that should handle the top/bottom of the list.

State::offsets/widths now just hold the *visible* items' offsets/widths. 

---

there's the bounds-relative Y coordinate system, stretching from 0 to bottom_y. the argument to refresh() refers to this system.

then there's the content-relative Y coordinate system, stretching from 0 to something > bottom_y. 

say we jumped to item E with offset_y...

  content  bounds
  
|--------|
|        |--------| <--- state.offset_y (scrolling offset)
|        |        |
|--------|--------| <--- offset_y
| E      |        |
|        |        |
|        |        |
|        |        |
|        |        |
|--------|        |
|        |--------|
|        |        
|--------|        

if there aren't enough items before E to fill it up, E.y - offset_y can be negative, so clamp to 0 on the lower end.

if there aren't enough items *after* E to put E at the desired bounds-relative Y offset, E.y - offset_y will put the scroll offset too low, so clamp it on the upper end.

state.offset_y = clamp(E.y - offset_y, 0.0, content.height - bounds.height)

which is the exact formula that State::scroll uses! how about that

---

on refresh(), we've got four possible cases:

- there are enough elements above and below to fill up the whole view
- there are enough elements above, but *not* enough below (towards end of list)
	- in this case, the desired offset_y is not achievable - must move it **down**
- there are enough elements below, but *not* enough above (towards beginning of list)
	- in this case, the desired offset_y is not achievable - must move it **up**
- there are not enough elements above or below (short list)
	- in this case, the desired offset_y is not achievable - **it's set to 0**
	
1. items before, if ran out,
	- slide offset up
2. items after. if ran out,
	- if first visible item is content.first(), 
		- we're done - set offset to 0 and return
	- otherwise,
		- slide offset down
		- items before, if ran out,
			- set offset to 0 and return

---

when adding an item there are a number of cases where it should be added and 

- there are no elements.
	- add it!
- it's adjacent to existing elements.
	- if it'd be onscreen, add it. (handles case where only a few elements in list)
- it's between existing elements.
	- add it - figure out what it's between to know Y, insert it, shift everything after it down