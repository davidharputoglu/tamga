//! Décompression du vieux format Unix « compress » (.Z, .tar.Z, .taz) — algorithme LZW.
//! Decoder for the classic Unix `compress` (.Z) format.
//!
//! Particularité du format : les codes sont écrits par groupes de 8 ; quand la
//! largeur des codes change (9 → 10 bits…) ou après un code CLEAR, le reste du
//! groupe en cours est du remplissage qu'il faut sauter.

use anyhow::{bail, Result};
use std::io::{self, Read};

const MAGIC: [u8; 2] = [0x1f, 0x9d];
const CLEAR: usize = 256;
const INIT_BITS: u32 = 9;

pub fn is_compress(head: &[u8]) -> bool {
    head.starts_with(&MAGIC)
}

/// Lecteur qui décompresse un flux .Z.
pub struct ZDecoder {
    data: Vec<u8>,
    pos: u64,   // position en bits dans `data`
    base: u64,  // début du groupe de codes de la largeur actuelle
    n_bits: u32,
    max_bits: u32,
    block_mode: bool,
    maxcode: usize,
    maxmaxcode: usize,
    free_ent: usize,
    prefix: Vec<u16>,
    suffix: Vec<u8>,
    oldcode: Option<usize>,
    finchar: u8,
    out: Vec<u8>,
    out_pos: usize,
    done: bool,
}

impl ZDecoder {
    pub fn new<R: Read>(mut r: R) -> Result<ZDecoder> {
        let mut data = Vec::new();
        r.read_to_end(&mut data)?;
        if data.len() < 3 || data[..2] != MAGIC {
            bail!("ce n'est pas un fichier .Z (compress)");
        }
        let flags = data[2];
        let max_bits = (flags & 0x1f) as u32;
        if !(9..=16).contains(&max_bits) {
            bail!("fichier .Z invalide ({max_bits} bits)");
        }
        let block_mode = flags & 0x80 != 0;
        let maxmaxcode = 1usize << max_bits;
        let start = 3 * 8;
        Ok(ZDecoder {
            data,
            pos: start,
            base: start,
            n_bits: INIT_BITS,
            max_bits,
            block_mode,
            maxcode: (1 << INIT_BITS) - 1,
            maxmaxcode,
            free_ent: if block_mode { 257 } else { 256 },
            prefix: vec![0; maxmaxcode],
            suffix: (0..maxmaxcode).map(|i| if i < 256 { i as u8 } else { 0 }).collect(),
            oldcode: None,
            finchar: 0,
            out: Vec::with_capacity(64 * 1024),
            out_pos: 0,
            done: false,
        })
    }

    /// Saute le reste du groupe de codes en cours.
    fn align(&mut self) {
        let group = (self.n_bits as u64) * 8;
        let used = self.pos - self.base;
        let rem = used % group;
        if rem != 0 {
            self.pos += group - rem;
        }
        self.base = self.pos;
    }

    fn read_code(&mut self) -> Option<usize> {
        let total_bits = self.data.len() as u64 * 8;
        if self.pos + self.n_bits as u64 > total_bits {
            return None;
        }
        let mut code = 0usize;
        for i in 0..self.n_bits as u64 {
            let bit = self.pos + i;
            let byte = self.data[(bit / 8) as usize];
            if byte >> (bit % 8) & 1 == 1 {
                code |= 1 << i;
            }
        }
        self.pos += self.n_bits as u64;
        Some(code)
    }

    /// Décode des codes jusqu'à avoir de la sortie (ou la fin).
    fn fill(&mut self) -> io::Result<()> {
        self.out.clear();
        self.out_pos = 0;
        while self.out.len() < 32 * 1024 {
            if self.free_ent > self.maxcode && self.n_bits < self.max_bits {
                self.align();
                self.n_bits += 1;
                self.maxcode = if self.n_bits == self.max_bits { self.maxmaxcode } else { (1 << self.n_bits) - 1 };
            }
            let Some(code) = self.read_code() else {
                self.done = true;
                break;
            };
            let Some(old) = self.oldcode else {
                if code > 255 {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "fichier .Z corrompu"));
                }
                self.finchar = code as u8;
                self.oldcode = Some(code);
                self.out.push(code as u8);
                continue;
            };
            if code == CLEAR && self.block_mode {
                self.align();
                self.n_bits = INIT_BITS;
                self.maxcode = (1 << INIT_BITS) - 1;
                self.free_ent = 256; // le prochain code ajouté sera 257
                self.oldcode = None;
                // Le code suivant est lu comme un premier code (caractère simple).
                let Some(c) = self.read_code_after_clear() else {
                    self.done = true;
                    break;
                };
                if c > 255 {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "fichier .Z corrompu"));
                }
                self.finchar = c as u8;
                self.oldcode = Some(c);
                self.out.push(c as u8);
                continue;
            }
            let incode = code;
            let mut stack: Vec<u8> = Vec::new();
            let mut c = code;
            if c >= self.free_ent {
                if c > self.free_ent {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "fichier .Z corrompu"));
                }
                stack.push(self.finchar);
                c = old;
            }
            while c >= 256 {
                stack.push(self.suffix[c]);
                c = self.prefix[c] as usize;
            }
            self.finchar = self.suffix[c];
            stack.push(self.finchar);
            self.out.extend(stack.iter().rev());
            if self.free_ent < self.maxmaxcode {
                self.prefix[self.free_ent] = old as u16;
                self.suffix[self.free_ent] = self.finchar;
                self.free_ent += 1;
            }
            self.oldcode = Some(incode);
        }
        Ok(())
    }

    fn read_code_after_clear(&mut self) -> Option<usize> {
        // Après CLEAR, free_ent vaut 256 : la largeur reste 9 bits.
        self.free_ent = 257;
        self.read_code()
    }
}

impl Read for ZDecoder {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.out_pos < self.out.len() {
                let n = (self.out.len() - self.out_pos).min(buf.len());
                buf[..n].copy_from_slice(&self.out[self.out_pos..self.out_pos + n]);
                self.out_pos += n;
                return Ok(n);
            }
            if self.done {
                return Ok(0);
            }
            self.fill()?;
        }
    }
}
