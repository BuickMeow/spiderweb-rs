//! 与 CPython `random.Random(seed)` 完全一致的 MT19937（tumour 的 random 方向需要逐位复刻，
//! 否则与 Python 生成的对照向量对不上）。

const N: usize = 624;
const M: usize = 397;
const MATRIX_A: u32 = 0x9908b0df;
const UPPER_MASK: u32 = 0x80000000;
const LOWER_MASK: u32 = 0x7fffffff;

pub struct PyRandom {
    mt: [u32; N],
    index: usize,
}

impl PyRandom {
    pub fn new(seed: i64) -> Self {
        let mut r = Self { mt: [0; N], index: N };
        r.seed(seed);
        r
    }

    fn seed(&mut self, seed: i64) {
        // CPython：取绝对值，按 32 位小端切成 key，再 init_by_array
        let mut n = seed.unsigned_abs();
        let mut key = Vec::new();
        while n > 0 {
            key.push((n & 0xffff_ffff) as u32);
            n >>= 32;
        }
        if key.is_empty() {
            key.push(0);
        }
        self.init_by_array(&key);
    }

    fn init_genrand(&mut self, s: u32) {
        self.mt[0] = s;
        for i in 1..N {
            self.mt[i] = 1812433253u32
                .wrapping_mul(self.mt[i - 1] ^ (self.mt[i - 1] >> 30))
                .wrapping_add(i as u32);
        }
        self.index = N;
    }

    fn init_by_array(&mut self, key: &[u32]) {
        self.init_genrand(19650218);
        let mut i = 1usize;
        let mut j = 0usize;
        let mut k = N.max(key.len());
        while k > 0 {
            self.mt[i] = (self.mt[i] ^ ((self.mt[i - 1] ^ (self.mt[i - 1] >> 30)).wrapping_mul(1664525)))
                .wrapping_add(key[j])
                .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= N {
                self.mt[0] = self.mt[N - 1];
                i = 1;
            }
            if j >= key.len() {
                j = 0;
            }
            k -= 1;
        }
        k = N - 1;
        while k > 0 {
            self.mt[i] = (self.mt[i] ^ ((self.mt[i - 1] ^ (self.mt[i - 1] >> 30)).wrapping_mul(1566083941)))
                .wrapping_sub(i as u32);
            i += 1;
            if i >= N {
                self.mt[0] = self.mt[N - 1];
                i = 1;
            }
            k -= 1;
        }
        self.mt[0] = 0x80000000;
    }

    pub fn genrand_uint32(&mut self) -> u32 {
        if self.index >= N {
            for kk in 0..N - M {
                let y = (self.mt[kk] & UPPER_MASK) | (self.mt[kk + 1] & LOWER_MASK);
                self.mt[kk] = self.mt[kk + M] ^ (y >> 1) ^ if y & 1 == 1 { MATRIX_A } else { 0 };
            }
            for kk in N - M..N - 1 {
                let y = (self.mt[kk] & UPPER_MASK) | (self.mt[kk + 1] & LOWER_MASK);
                self.mt[kk] = self.mt[kk - (N - M)] ^ (y >> 1) ^ if y & 1 == 1 { MATRIX_A } else { 0 };
            }
            let y = (self.mt[N - 1] & UPPER_MASK) | (self.mt[0] & LOWER_MASK);
            self.mt[N - 1] = self.mt[M - 1] ^ (y >> 1) ^ if y & 1 == 1 { MATRIX_A } else { 0 };
            self.index = 0;
        }
        let mut y = self.mt[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c5680;
        y ^= (y << 15) & 0xefc60000;
        y ^= y >> 18;
        y
    }

    /// CPython getrandbits(k)（k <= 32）。
    pub fn getrandbits(&mut self, k: u32) -> u32 {
        self.genrand_uint32() >> (32 - k)
    }

    /// CPython _randbelow_with_getrandbits(n)。
    pub fn randbelow(&mut self, n: u32) -> u32 {
        let k = 32 - n.leading_zeros();
        let mut r = self.getrandbits(k);
        while r >= n {
            r = self.getrandbits(k);
        }
        r
    }

    /// 长度 2 的元组里随机取一个（tumour 的 "random" 方向）。
    pub fn choice2(&mut self) -> f64 {
        if self.randbelow(2) == 0 { 1.0 } else { -1.0 }
    }

    /// CPython random()。
    pub fn random(&mut self) -> f64 {
        let a = (self.genrand_uint32() >> 5) as f64;
        let b = (self.genrand_uint32() >> 6) as f64;
        (a * 67108864.0 + b) * (1.0 / 9007199254740992.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_cpython() {
        // 与 python3 对照：random.Random(1) 的前几个 getrandbits(2)
        let mut r = PyRandom::new(1);
        let got: Vec<u32> = (0..8).map(|_| r.getrandbits(2)).collect();
        // python3 -c "import random; r=random.Random(1); print([r.getrandbits(2) for _ in range(8)])"
        assert_eq!(got, vec![0, 2, 3, 3, 3, 0, 1, 0]);
        let mut r = PyRandom::new(42);
        let choices: Vec<f64> = (0..16).map(|_| r.choice2()).collect();
        // python3 -c "import random; r=random.Random(42); print([r.choice((1,-1)) for _ in range(16)])"
        assert_eq!(
            choices,
            vec![1.0, 1.0, -1.0, 1.0, 1.0, 1.0, 1.0, 1.0, -1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0]
        );
    }
}
