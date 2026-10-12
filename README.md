# Harvey MIE

This repository locates, strips, and appends a Meta Information Encapsulation trailer, and it reads and writes only one document. That document is a big-endian file-level `0MIE` group, one UTF-8 element named `Pin`, and the empty `zmie` signature. It is not a general MIE reader or writer.

`MIE` is the library. `harvey-mie` is a Windows window that edits that `Pin` text in a JPEG and writes the same bytes to the sibling `.jpg.json`. The window does not open an arbitrary MIE document.

The MIE format and ExifTool's MIE module are copyright Phil Harvey. These Rust sources follow ExifTool's trailer rules and are distributed under the same terms: the Perl Artistic License or the GNU General Public License version 3. Each crate has `LICENSE-ARTISTIC` and `LICENSE-GPL`.
