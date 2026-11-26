# Remote authentication

The Git Engine accepts HTTPS, `ssh://[user@]host[:port]/path` and `user@host:path` endpoints. SSH accepts conventional ASCII usernames, hostnames and repository paths containing letters, digits, slashes, periods, underscores and hyphens. Provider-specific login flows are outside this transport interface.

## HTTPS on Linux

Use **Anonymous** for a public endpoint. For a private endpoint, select the Git memory cache, the system Secret Service or **Password or token for this operation**.

- The memory cache and Secret Service require the corresponding system-installed Git helpers. The engine looks for trusted system binaries called `git-credential-cache` and `git-credential-libsecret`. A provider can require an account username paired with a scoped token, and a service may refuse a password for Git.
- Manual credentials apply to one explicitly confirmed operation. The password field is masked. Git reads a derived Basic authorization header through a scoped child environment variable. The process arguments contain the variable name. The application never persists the password or token. As with other process environment data, a sufficiently privileged local process may be able to inspect the child.
- TLS verification is mandatory. A server with a private CA can be accessed by explicitly selecting a trusted PEM CA bundle. The selected file must be a regular file. HTTP redirects are refused, including redirects to a different host.

Credentials embedded in HTTPS URLs are rejected. The app suppresses the unfiltered diagnostics from networking commands. For an authentication failure, check the credential, expiry and repository permissions, then explicitly retry.

## SSH on Linux

The native SSH entry point runs the trusted system OpenSSH client with strict host key checking. It uses the known hosts databases or the file selected in the confirmation dialog. If a host is unknown or its key changes, confirm the fingerprint with the hosting service or server administrator before updating the trusted file outside the app. Host keys are never enrolled automatically.

The client uses default OpenSSH identities, an explicit identity file, or an existing SSH agent. Load encrypted keys with `ssh-add` before starting the operation. The application does not read private keys or prompt for a passphrase. The client uses known identities and agent sockets without forwarding the agent. An explicit identity and known hosts file are passed only to the client. Host aliases, proxy commands and OpenSSH startup configuration are disabled. Enter the hostname and optional port directly.

[Git credential helpers](https://git-scm.com/docs/gitcredentials) · [Git HTTPS configuration](https://git-scm.com/docs/git-config#Documentation/git-config.txt-httpsslVerify) · [OpenSSH client options](https://man.openbsd.org/ssh_config#StrictHostKeyChecking)
