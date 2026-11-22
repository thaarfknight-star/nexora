# Nexora-MT — Custom From-Scratch Offline Translation Model

[![Open In Colab](https://colab.research.google.com/assets/colab-badge.svg)](https://colab.research.google.com/github/thaarfknight-star/nexora/blob/main/nexora-mt/train_nexora_mt_colab.ipynb)

`Nexora-MT v1` is Nexora's own lightweight (~30M-parameter) Encoder-Decoder Transformer (Pre-Norm, RMSNorm, SwiGLU, weight-tied embeddings, custom 16k bilingual BPE tokenizer) designed specifically for real-time offline English → Persian subtitle translation (~35–42 MB quantized `.nxmt` binary, <15 ms CPU latency, 0% GPU usage).

## Training on Google Colab (Free T4 GPU)

1. Click the **Open In Colab** badge above.
2. In Colab, select **Runtime → Change runtime type → T4 GPU**.
3. Click **Runtime → Run all**.
   - Checkpoints automatically save to your Google Drive (`MyDrive/NexoraMT/`) every epoch so you can safely resume if Colab disconnects.
4. After training completes, Cell 7 exports **`nexora-mt-en-fa-v1.nxmt`** (~35–42 MB) to your Google Drive and downloads it to your PC.
5. Upload `nexora-mt-en-fa-v1.nxmt` to a GitHub Release on `thaarfknight-star/nexora` (e.g. tag `nexora-mt-v1`) so Nexora's native Rust inference engine can download and run it offline for every user.
