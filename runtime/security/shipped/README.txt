Matching source for the currently bundled QEMU binaries

tpm-qemu.c is the exact GPL-2.0-or-later source recorded in both bundled
QEMU SOURCE_BUILD.json files, SHA-256:
8d6d0a3b80cc9c49a54bb00d66f850f50208f34bd7a452a1fec78a52ffa54e92

It was recovered from repository commit 45f6403 and checked against that
recorded hash. The adjacent development source ../tpm-qemu.c has newer
Yougori display strings. Existing binaries still use the earlier strings.
Use this retained file when rebuilding the currently bundled binaries;
use the development file for a new build with new provenance records.

The existing runtime/security/LICENSE GPL terms apply to this copy.
