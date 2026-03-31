use leptess::{LepTess, Variable};
use ocrs::{OcrEngine, OcrEngineParams};
use rten::Model;
use tokio::sync::mpsc;

pub const DETECTION_MODEL: &[u8] = include_bytes!("../models/text-detection.rten");
pub const RECOGNITION_MODEL: &[u8] = include_bytes!("../models/text-recognition.rten");

// --- LepTess Pool ---

pub struct LepTessPool {
    sender: mpsc::Sender<LepTess>,
    receiver: tokio::sync::Mutex<mpsc::Receiver<LepTess>>,
}

impl LepTessPool {
    pub fn new(size: usize) -> Self {
        let (sender, receiver) = mpsc::channel::<LepTess>(size);
        for _ in 0..size {
            let lt = create_leptess();
            sender
                .try_send(lt)
                .expect("Failed to populate LepTess pool");
        }
        Self {
            sender,
            receiver: tokio::sync::Mutex::new(receiver),
        }
    }

    pub async fn acquire(&self) -> LepTess {
        self.receiver
            .lock()
            .await
            .recv()
            .await
            .expect("LepTess pool exhausted")
    }

    pub async fn release(&self, lt: LepTess) {
        let _ = self.sender.send(lt).await;
    }

    pub async fn replenish(&self) {
        let lt = create_leptess();
        let _ = self.sender.send(lt).await;
    }
}

fn create_leptess() -> LepTess {
    let mut lt = LepTess::new(None, "fra").expect("Failed to initialize LepTess");
    lt.set_variable(Variable::TesseditPagesegMode, "12")
        .expect("Failed to set PSM");
    lt.set_variable(Variable::PreserveInterwordSpaces, "1")
        .expect("Failed to set preserve_interword_spaces");
    lt
}

// --- OcrEngine Pool ---

pub struct OcrEnginePool {
    sender: mpsc::Sender<OcrEngine>,
    receiver: tokio::sync::Mutex<mpsc::Receiver<OcrEngine>>,
}

impl OcrEnginePool {
    pub fn new(size: usize) -> Self {
        let (sender, receiver) = mpsc::channel::<OcrEngine>(size);
        for _ in 0..size {
            let engine = create_ocr_engine();
            sender
                .try_send(engine)
                .expect("Failed to populate OcrEngine pool");
        }
        Self {
            sender,
            receiver: tokio::sync::Mutex::new(receiver),
        }
    }

    pub async fn acquire(&self) -> OcrEngine {
        self.receiver
            .lock()
            .await
            .recv()
            .await
            .expect("OcrEngine pool exhausted")
    }

    pub async fn release(&self, engine: OcrEngine) {
        let _ = self.sender.send(engine).await;
    }

    pub async fn replenish(&self) {
        let engine = create_ocr_engine();
        let _ = self.sender.send(engine).await;
    }
}

fn create_ocr_engine() -> OcrEngine {
    let detection_model = Model::load_static_slice(DETECTION_MODEL).unwrap();
    let recognition_model = Model::load_static_slice(RECOGNITION_MODEL).unwrap();

    OcrEngine::new(OcrEngineParams {
        detection_model: Some(detection_model),
        recognition_model: Some(recognition_model),
        ..Default::default()
    })
    .unwrap()
}
