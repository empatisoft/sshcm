# Security

sshcm edits your `~/.ssh/config` and starts `ssh`. It does not read keys,
passwords or passphrases, opens no network connections and collects nothing.

## Reporting a vulnerability

Please do not open a public issue for a security problem. Report it privately
through GitHub: open
[github.com/empatisoft/sshcm/security/advisories/new](https://github.com/empatisoft/sshcm/security/advisories/new)
("Report a vulnerability" on the repository's Security tab).

Things worth reporting include: a way to make sshcm write outside the config
files it manages, to run a command other than the system `ssh` (or `nc -h`,
which it runs to check for proxy support), to pass attacker-controlled options
to `ssh`, or to leave the config in a state ssh rejects.

Only the latest release is supported. Fixes are published as a new release.
