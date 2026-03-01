/**
 * Compute the playhead seek position for a similarity chunk match.
 * Finds the first MIDI note at or after the chunk offset, then backs up 0.3s
 * so the listener hears the lead-in rather than starting mid-note.
 */
export function computeChunkSeekTime(
    matchOffsetSecs: number | null,
    duration: number,
    notes: Array<{ time: number }>,
): number {
    const chunkStart =
        matchOffsetSecs != null && matchOffsetSecs > 0 && matchOffsetSecs < duration
            ? matchOffsetSecs
            : 0;
    const firstNote = notes.find((n) => n.time >= chunkStart);
    return firstNote ? Math.max(0, firstNote.time - 0.3) : chunkStart;
}
