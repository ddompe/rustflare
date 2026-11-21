# Signed apt repository

With the repository, a Pi installs and upgrades RustFlare with plain `apt`, and every package is checked against your signature (the SHA-256 check in `rustflare-update` only protects against corruption, not against a tampered release).

## One-time setup (you, about 10 minutes)

1. **Make a signing key** on your Mac (keep the private key safe; it can sign anything as "RustFlare"):

   ```
   gpg --quick-generate-key "RustFlare apt <your-email>" ed25519 sign 5y
   gpg --list-secret-keys --keyid-format long          # note the long key id (after ed25519/)
   ```

   Choose a passphrase when asked (or none; then leave the passphrase secret empty).

2. **Add it to the GitHub repository** (Settings > Secrets and variables > Actions > New repository secret):

   - `APT_GPG_PRIVATE_KEY`: the output of `gpg --armor --export-secret-keys <keyid>`
   - `APT_GPG_PASSPHRASE`: the passphrase (create the secret even if empty)

   Back the private key up somewhere offline (`gpg --armor --export-secret-keys <keyid> > rustflare-apt-key.asc`, then keep it in your password manager). Delete the file from the disk you do not trust.

3. **Turn on Pages**: Settings > Pages > Source: **GitHub Actions**.

4. **Publish**: push a release tag (`git tag v0.2.7 && git push origin v0.2.7`, matching the version in `Cargo.toml`). The *Release* workflow builds the `.deb`; the *Apt repository* workflow then rebuilds and signs the repository and deploys it to `https://ddompe.github.io/rustflare/`. You can also start *Apt repository* by hand from the Actions tab.

## On each Pi

```
curl -fsSL https://ddompe.github.io/rustflare/rustflare.gpg | sudo tee /usr/share/keyrings/rustflare.gpg >/dev/null
echo "deb [signed-by=/usr/share/keyrings/rustflare.gpg arch=arm64] https://ddompe.github.io/rustflare stable main" | sudo tee /etc/apt/sources.list.d/rustflare.list
sudo apt update && sudo apt install rustflare
```

Afterwards `sudo apt update && sudo apt upgrade` updates RustFlare along with the system. As before, an update restarts the service, so do not upgrade in the middle of an observation. `rustflare-update` keeps working too.

## Rotating the key

Generate a new key, replace the two secrets, run *Apt repository*, and on each Pi replace `/usr/share/keyrings/rustflare.gpg` with the new public key.
