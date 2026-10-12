# mie

This library locates, strips, and appends a Meta Information Encapsulation trailer, and it reads and writes only one document. That document is a big-endian file-level `0MIE` group, one UTF-8 element named `Pin`, and the empty `zmie` signature. It is not a general MIE reader or writer.

The MIE format and ExifTool's MIE module are copyright Phil Harvey. These Rust sources follow ExifTool's trailer rules and are distributed under the same terms: the Perl Artistic License (`LICENSE-ARTISTIC`) or the GNU General Public License version 3 (`LICENSE-GPL`).

## Functions

- `ends_with_signature`, `group_length`, and `locate` find a trailer frame.
- `strip` drops a signed trailer. A host with no trailer is unchanged.
- `append` adds a document that already ends with `zmie`.
- `document` builds the one `Pin` document.
- `embed` strips, then appends `document`.
- `pin` returns that `Pin` slice, or nothing.

`pin` returns nothing for any other shape, including a little-endian group, an extra element, or a missing `Pin`.
