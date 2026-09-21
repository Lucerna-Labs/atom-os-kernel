# Rejected v3 diagnostic pack

This local pack passed its training metrics but was rejected during VM
acceptance because the production verifier imported the training module, whose
top-level NumPy dependency was unavailable in the intentionally minimal VM.

The failure is packaging/verification portability, not evidence of accelerator
training. It was never submitted, embedded as an accepted release, or sealed.
The corrected verifier loads validation code without NumPy; the active v3
diagnostic pack was retrained so its receipt binds the corrected trainer hash.
