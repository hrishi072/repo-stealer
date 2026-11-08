Experimental CLI milestone: encrypted storage, same-identity backups and full-history local Git capture/restore. Git commands require installed Git 2.48+ and verified encrypted working storage: ext4/XFS on LUKS for Linux, EFS for Windows. Restores create verified new bare repositories. Provider migrations and a desktop interface remain future work. LFS payloads, submodule repositories and hosting metadata are outside Git snapshot scope.

Assets include Linux x86_64 DEB, RPM, AppImage and portable archive, plus Windows
x86_64 EXE and ZIP. AppImage is a terminal application. Windows binaries are not
Authenticode signed; release checksums are provided. Git commits and release tags
use the maintainer's SSH signing key.

Use a strong passphrase and retain a verified backup. Vaults and backups share
the underlying key; losing both passphrase and unlocked key access loses recovery.
The format is experimental and has not had an independent application security
review. See README and docs/SECURITY.md before storing important data.
