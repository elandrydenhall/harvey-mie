# Harvey MIE

Harvey MIE edits that `Pin` text in a JPEG and writes the same bytes to the sibling `.jpg.json`. It does not open an arbitrary MIE document. The window is Windows-only because save uses `MoveFileExW`. The `mie` library itself has no Win32 calls.

The MIE format and ExifTool's MIE module are copyright Phil Harvey. These Rust sources follow ExifTool's trailer rules and are distributed under the same terms: the Perl Artistic License (`LICENSE-ARTISTIC`) or the GNU General Public License version 3 (`LICENSE-GPL`).

The window opens with an empty folder field. Type a folder of JPEG files and press Open.

Save is enabled only when the text has changed and is valid JSON. The bytes written are the editor text. A read-back mismatch restores the JPEG and leaves the sibling JSON unchanged. The program does not recompute `image_hash` or any other field.
