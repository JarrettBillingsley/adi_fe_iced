
3. have it determine item visibility based on its own scrolling
4. have it only manifest items on-demand, instead of manifesting everything up front

domain == 10
3 items

0    0.0
3    20.0
5    40.0
7    40.0   <-- prev_offset == 40.0
10   60.0

add_offset(idx = 5, height = 15)

---

know the topmost index and its offset off the top of the screen

- its offset off the top of the screen is State::offset_y, already tracking that.
- topmost index is also already tracked, visible_layouts.first().0

have a buffer size **in pixels** that we maintain above/below current view. measured in pixels cause scrolling happens in pixels, not items; items might be short or tall and as a result the rate at which items are needed changes even with the same scrolling speed.

probably also good to know the **first actually-visible item** and its offset off the top of the screen (<= 0), so that if there are changes onscreen, the view stays in the same place

- any changes to items before that (in the offscreen buffer) should *not* move the view.

before we scroll, we ask, "would scrolling by this much put us in the danger zone?" so:

- when scrolling down:
	- if bottom-most manifested element's bottom moves **above bottom + buffer size,**
		- append more items until bottom is >= bottom + buffer size
	- if top-most manifested element's bottom moves **above top - buffer size,**
		- remove items at start until top-most bottom is below top - buffer size
		- each removal also needs to adjust the scroll position!
- when scrolling up, do reverse
	- prepend items as needed (while adjusting scroll position for each one)
	- remove items at end as needed

the State::scroll() method already handles the clamping of the scroll at the content bounds, so that should handle the top/bottom of the list.

**jumping to an item** should be easy: set the first actually-visible item to the jumped-to item with a scroll offset of 0, and then append/prepend items until the safe zones are filled in (unless we hit the start/end of the list, in which case stuff might need to be slid around)

State::offsets/widths now just hold the *visible* items' offsets/widths. 

---

I think I'm mixing up 2 Y coordinate systems.

there's the bounds-relative Y coordinate system, stretching from 0 to bottom_y. the argument to refresh() refers to this system.

then there's the content-relative Y coordinate system, stretching from 0 to something > bottom_y. 

currently I'm laying out the elements sort of in the content-relative space? but with potentially negative coordinates? which is wrong

we *do* need to spawn elements before the requested one if offset_y > 0, and it *does* tell us how tall they all need to be. but their actual y positions should be 0 and up.

what offset_y *should* be used for is for determining the overall scroll position. it is the bounds-relative Y coordinate at which the top of the jumped-to item should appear (if possible).

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

