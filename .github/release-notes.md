Experimental CLI milestone: encrypted local object storage, encrypted catalog,
same-identity portable backups, offline verification and restore. Git transfers
and a desktop interface are not implemented in this release.

Assets include Linux x86_64 DEB, RPM, AppImage and portable archive, plus Windows
x86_64 EXE and ZIP. AppImage is a terminal application. Windows binaries are not
Authenticode signed; release checksums are provided. Git commits and release tags
use the maintainer's SSH signing key.

Use a strong passphrase and retain a verified backup. Vaults and backups share
the underlying key; losing both passphrase and unlocked key access loses recovery.
The format is experimental and has not had an independent application security
review. See README and docs/SECURITY.md before storing important data.
