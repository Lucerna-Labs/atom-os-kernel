# Private TPU payload

The user authorized a private Kaggle upload after the local gate passed. The v3
gate is recorded in ../receipts/pre-tpu-v3-20260921.md.

Generate the upload only from a clean committed revision:

    python3 network-lightcone/tools/prepare_kaggle.py \
      --output target/network-lightcone/kaggle-v3

The generator verifies that the world and trainer match HEAD, embeds only those
two files, and writes exact hashes to UPLOAD-MANIFEST.json. It includes no
packet captures, credentials, private user data, or sibling trained worlds.
The Kaggle program requires actual TPU devices. A completed run is accelerator
training evidence only; it does not automatically authorize a seal.
