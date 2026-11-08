# Retained verification outputs

Raw test output and snapshots are preserved byte for byte in [CHS-P06-verification-outputs.tar.gz](CHS-P06-verification-outputs.tar.gz), with a [CHS-P06-verification-outputs.json](CHS-P06-verification-outputs.json) checksum index. Extract from the phase docs directory to restore the original `logs/` paths. The working checkout also retains the raw files locally. This packaging preserves patch/progress whitespace without exempting source or human documentation from whitespace checks.

The accepted source archives and scoped source manifest remain separate. Full combined P07 source/diff/merge/backup evidence containing preserved unrelated inputs stays local; the committed P07 scoped source archive contains the exact owned code and original-dependency browser hunks verified for the commit.
