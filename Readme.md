# Revorbis-rs

A try to rewrite libvorbis in Rust.

## My main target

Firstly I'm not going to reinvent the Vorbis wheel, I just want to use `libvorbis` to encode/decode the Ogg Vorbis audio files.

But one day, I had to face the non-capsulated naked Vorbis file. `libvorbis` fails to decode this kind of audio, it must need the Ogg encapsulation or Matroska.

I've tried to re-encapsulate the naked Vorbis file using `Ogg`, but then I found out that I need two things:
1. The Vorbis packet length, which can only be obtained by decoding the bitwise Vorbis packet.
2. The granule position for each Vorbis packet.

So I'm now re-inventing the Vorbis wheel in Rust.

Currently I've done parsing the Vorbis headers:

```rust
#[derive(Debug, Default, Clone, PartialEq)]
pub struct VorbisSetupHeader {
    pub static_codebooks: CodeBooks,
    pub floors: Vec<VorbisFloor>,
    pub residues: Vec<VorbisResidue>,
    pub maps: Vec<VorbisMapping>,
    pub modes: Vec<VorbisMode>,
}
```
