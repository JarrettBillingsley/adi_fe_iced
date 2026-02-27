
1. automatic spawning/despawning on scrolling
2. item_changed/removed/added can also cause elements to need to be spawned/despawned
3. also when resized?? have to detect that based on uhhh bounds in draw()..
	- currently resizing it causes it to black out, wtf?
4. State::size should really be named like `content_size` or `content_bounds` or something *cause that's what it is*

---

before we scroll, we ask, "would scrolling by this much put us in the danger zone?" so:

- when scrolling down:
	- if bottom-most manifested element's bottom moves **above bottom,**
		- append more items until bottom is >= bottom
	- if top-most manifested element's bottom moves **above top,**
		- remove items at start until top-most bottom is below top
		- each removal also needs to adjust the scroll position!
- when scrolling up, do reverse
	- prepend items as needed (while adjusting scroll position for each one)
	- remove items at end as needed

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