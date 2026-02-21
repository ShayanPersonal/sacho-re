This document describes the proposed similarity tab in Sacho.

For now, the similarity tab does not work on in-app recordings. Instead, the tab prompts the user to import a folder of MIDI files. Later it will work on in-app recordings, but we are doing it this way first to test with our pre-existing datasets.

User experience:
On import, a scrollable listing of all the midi files appears on the left side of the screen. Clicking on a midi file brings up the MIDI file in the 2D visualization surrounded by similar files. The visualizations are fun and interesting. When a midi file is selected, a center node representing that file appears in the center of the screen, and smaller nodes rush out of the center node but deaccelerate and stop before going out too far, but leaving some space between themselves. the smaller nodes represent the (up to) 12 most similar files (files that are dissimilar past some threshold aren't included). The closest matching nodes should be closer to the center node, but the actual distances do not need to correlate with the computed by the backend. their distances can correlate with rank, with the closest node visually having had the closest value.

When a smaller node is clicked on, that node becomes the new center node, and just like before it spawns a new set of most similar nodes.

Similarity scoring:
