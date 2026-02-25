
1. get the SparseList using IContent instead of Content directly
2. make sure it doesn't make assumptions about density of indices
3. have it handle scrolling itself, instead of determining item visibility based on viewport
4. have it only manifest items on-demand, instead of manifesting everything up front

domain == 10
3 items

0    0.0
3    20.0
5    40.0
7    40.0   <-- prev_offset == 40.0
10   60.0

add_offset(idx = 5, height = 15)