//! Pool d'instances Tesseract in-process (leptess).
//!
//! Une instance `LepTess` charge le modèle `fra` (plusieurs dizaines de Mo) : on ne
//! l'initialise pas à chaque appel, on la réutilise. Le pool est global et synchrone
//! parce que Tesseract est appelé au fond de la cascade OCR, sur les threads bloquants,
//! loin de la couche HTTP. Chaque appel emprunte une instance le temps d'une
//! reconnaissance et la rend aussitôt (RAII) : une analyse abandonnée sur timeout ne
//! garde donc pas d'instance au-delà de l'appel en cours.
//!
//! Les instances sont créées à la demande, jusqu'à la capacité du pool ; au-delà,
//! l'appelant attend qu'une instance se libère. Une instance utilisée pendant une
//! panique est jetée (son état n'est plus sûr) et sera recréée au besoin.
//!
//! PP-OCR, l'autre moteur, n'a pas besoin de pool : une session ONNX Runtime est
//! partagée entre threads (voir `ppocr.rs`).

use std::ops::{Deref, DerefMut};
use std::sync::{Condvar, Mutex, OnceLock};

use leptess::{LepTess, Variable};

use crate::config::AppConfig;

static LEPTESS_POOL: OnceLock<LepTessPool> = OnceLock::new();

/// Fixe la capacité du pool global. Sans effet si le pool est déjà initialisé ; sans
/// appel, la capacité est `LA_TAUPE_MAX_CONCURRENT_OCR`.
pub fn init_leptess_pool(capacity: usize) {
    let _ = LEPTESS_POOL.set(LepTessPool::new(capacity));
}

/// Emprunte une instance Tesseract au pool global, rendue quand la garde est lâchée.
pub fn leptess() -> PooledLepTess<'static> {
    LEPTESS_POOL
        .get_or_init(|| LepTessPool::new(AppConfig::from_env().max_concurrent_ocr))
        .acquire()
}

pub struct LepTessPool {
    state: Mutex<PoolState>,
    available: Condvar,
    capacity: usize,
}

struct PoolState {
    idle: Vec<LepTess>,
    created: usize,
}

impl LepTessPool {
    pub fn new(capacity: usize) -> Self {
        Self {
            state: Mutex::new(PoolState {
                idle: Vec::new(),
                created: 0,
            }),
            available: Condvar::new(),
            capacity: capacity.max(1),
        }
    }

    pub fn acquire(&self) -> PooledLepTess<'_> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(lt) = state.idle.pop() {
                return PooledLepTess {
                    pool: self,
                    lt: Some(lt),
                };
            }
            if state.created < self.capacity {
                state.created += 1;
                drop(state);
                return match create_leptess() {
                    Ok(lt) => PooledLepTess {
                        pool: self,
                        lt: Some(lt),
                    },
                    Err(e) => {
                        self.forget_one();
                        panic!("Failed to initialize LepTess: {}", e);
                    }
                };
            }
            state = self
                .available
                .wait(state)
                .unwrap_or_else(|e| e.into_inner());
        }
    }

    fn give_back(&self, lt: LepTess) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.idle.push(lt);
        self.available.notify_one();
    }

    /// Une instance perdue libère sa place : la prochaine demande en recréera une.
    fn forget_one(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.created -= 1;
        self.available.notify_one();
    }

    #[cfg(test)]
    fn created(&self) -> usize {
        self.state.lock().unwrap().created
    }
}

pub struct PooledLepTess<'a> {
    pool: &'a LepTessPool,
    lt: Option<LepTess>,
}

impl Deref for PooledLepTess<'_> {
    type Target = LepTess;

    fn deref(&self) -> &LepTess {
        self.lt.as_ref().expect("LepTess already returned")
    }
}

impl DerefMut for PooledLepTess<'_> {
    fn deref_mut(&mut self) -> &mut LepTess {
        self.lt.as_mut().expect("LepTess already returned")
    }
}

impl Drop for PooledLepTess<'_> {
    fn drop(&mut self) {
        if let Some(lt) = self.lt.take() {
            if std::thread::panicking() {
                drop(lt);
                self.pool.forget_one();
            } else {
                self.pool.give_back(lt);
            }
        }
    }
}

fn create_leptess() -> Result<LepTess, String> {
    let mut lt = LepTess::new(None, "fra").map_err(|e| e.to_string())?;
    lt.set_variable(Variable::TesseditPagesegMode, "12")
        .map_err(|e| e.to_string())?;
    lt.set_variable(Variable::PreserveInterwordSpaces, "1")
        .map_err(|e| e.to_string())?;
    Ok(lt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn reuses_an_instance_instead_of_creating_one() {
        let pool = LepTessPool::new(2);
        drop(pool.acquire());
        drop(pool.acquire());
        assert_eq!(pool.created(), 1);
    }

    #[test]
    fn never_creates_more_than_its_capacity() {
        let pool = Arc::new(LepTessPool::new(1));
        let held = pool.acquire();

        let waiter = {
            let pool = pool.clone();
            thread::spawn(move || {
                let _lt = pool.acquire();
            })
        };
        thread::sleep(Duration::from_millis(100));
        assert!(!waiter.is_finished(), "the second caller must wait");
        assert_eq!(pool.created(), 1);

        drop(held);
        waiter.join().unwrap();
        assert_eq!(pool.created(), 1);
    }

    #[test]
    fn an_instance_used_during_a_panic_is_discarded() {
        let pool = Arc::new(LepTessPool::new(1));
        {
            let pool = pool.clone();
            let _ = thread::spawn(move || {
                let _lt = pool.acquire();
                panic!("ocr failure");
            })
            .join();
        }
        assert_eq!(pool.created(), 0);
        drop(pool.acquire());
        assert_eq!(pool.created(), 1);
    }
}
