This document describes the proposed similarity tab in Sacho. There is some existing stub / skeleton code but it should be completely removed and replaced.

For now, the proposed similarity tab should not work on in-app recordings. Instead, the tab prompts the user to import a folder of MIDI files. Later it will work on in-app recordings, but we are doing it this way first to test with our pre-existing datasets.

User experience:
On import, a scrollable listing of all the midi files appears on the left side of the screen. Clicking on a midi file brings up the MIDI file in the 2D visualization surrounded by similar files. The visualizations are fun and interesting. When a midi file is selected, a center node representing that file appears in the center of the screen, and smaller nodes rush out of the center node but deaccelerate and stop before going out too far, but leaving some space between themselves. the smaller nodes represent the (up to) 12 most similar files (files that are dissimilar past some threshold aren't included). The closest matching nodes should be closer to the center node, but the actual distances do not need to correlate with the computed by the backend. their distances can correlate with rank, with the closest node visually having had the closest value.

When a smaller node is clicked on, that node becomes the new center node, and just like before it spawns a new set of most similar nodes.

Similarity scoring:
MIDI files may be simple one-note-at-a-time performances or they may be complex recordings of a full-sized keyboard, with 10 or more active notes at a time. We won't know, and your algorithm should generalize to all sorts of files. Only consider note-on and optionally note-off events for scoring (E.G. if you determine note durations could matter)

There should be two types of similarity scores computer separately: melodic similarity, and harmonic similarity. In the ui, the user can switch between them, and it'll draw / redraw nodes based on the new scoring.

Looks like this website has information on latest research in determining melodic and/or harmonic similarity: https://music-ir.org/mirex/wiki/MIREX_HOME

here's an algorithm from 2015, there may be better ways to do melody similarity now (this assumes the input is monophonic for example) https://github.com/julian-urbano/MelodyShape

For similarity scoring, plan out how to do accurate melodic and harmonic similarity matching.
