# JavaScript dependency policy

This project requires npm 11.18.0; `devEngines` fails installs, CI, and package
scripts under a different package-manager version. Direct `dependencies` and
`devDependencies` stay on the npm `latest` tag so an intentional lockfile
refresh considers the current release line. The project `.npmrc` applies
`min-release-age=2`, so npm will only select releases that have been public for
at least 48 hours.

`package-lock.json` is the exact, reviewed dependency graph. Normal and CI
installs use `npm ci`; do not delete or bypass the lockfile.

The current lockfile selects TypeScript 6.0.3 even though its manifest selector
remains `latest`. That is the newest version accepted by the peer ranges of the
locked SvelteKit 2.70.2 and `svelte-check` 4.7.5 releases; both currently exclude
TypeScript 7. A deliberate refresh must not force an incompatible peer graph to
make an `outdated` report empty. Remove this compatibility hold when stable
Svelte tooling admits TypeScript 7 and the full verification suite passes.

The latest locked jsdom requires Node.js 24.15.0 or newer on the Node 24 line
(or a supported Node 22/26 line). Runtime users do not ship jsdom, but console
contributors should use Node 24.15+ so `npm ci` does not emit an engine warning.

Dependency lifecycle scripts are denied by default. `strict-allow-scripts`
makes an install fail when an unreviewed package requests an install script.
There are currently no approved exceptions. The optional `fsevents` installer
is explicitly denied because the console does not require it to build, check,
or test. If another script becomes necessary, inspect the exact package release
and script first, then approve only that package with:

```sh
npm install-scripts approve <package>
```

npm pins approvals to the reviewed version. Do not use `approve --all` or
`dangerously-allow-all-scripts`.

For a deliberate refresh, run `npm update --package-lock-only`, inspect both
manifest and lockfile changes, then verify with `npm ci`, `npm audit`,
`npm run check`, `npm test`, and `npm run build`. The current full audit reports
three low-severity findings in the development-only static-adapter → SvelteKit
→ `cookie` chain; `npm audit --omit=dev` reports zero production findings.
