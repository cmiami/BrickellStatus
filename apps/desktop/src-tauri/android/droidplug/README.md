# droidplug — vendored Java companion for btleplug

`java/` is a verbatim copy of `src/droidplug/java/src/main/java/` from the
[btleplug](https://github.com/deviceplug/btleplug) crate, at the version this
workspace locks in `Cargo.lock`.

## Why it is committed rather than fetched

btleplug's Android backend is a hybrid Rust/Java build. The Rust side reaches
Android's Bluetooth stack through JNI, and the classes it looks up have to be
present in the APK's own classpath. Those classes are **not published to Maven** —
they ship inside the crate as a separate Gradle library project, which would
mean building a second AAR during our build. Copying the sources into the app's
`main` source set does the same job with no extra Gradle project.

The two package trees are both required:

- `com/nonpolynomial/btleplug/android/impl/` — the Bluetooth adapter, peripheral
  and exception types btleplug calls.
- `io/github/gedgygedgy/rust/` — the `jni-utils` support classes (futures,
  streams, wakers) those depend on. They are bundled inside btleplug rather than
  being a separate dependency to resolve.

## Keeping it in step with the crate

`apps/console/scripts/sync-droidplug-java.mjs` re-copies these files from the
resolved crate source, and `--check` fails when the tree has drifted. CI runs the
check, so a `cargo update` that moves btleplug cannot silently leave the Java
behind while the Rust moves on — a mismatch there surfaces as a `NoSuchMethodError`
at runtime on a device, which is a miserable thing to debug.

To refresh after a deliberate btleplug bump:

```bash
node apps/console/scripts/sync-droidplug-java.mjs
```

## Build wiring

`gen/android/app/build.gradle.kts` adds this directory as an extra source set,
and `gen/android/app/proguard-rules.pro` keeps both package trees — every class
here is reached only through JNI, so R8 would otherwise strip the lot.

## Licence

btleplug is `MIT/Apache-2.0/BSD-3-Clause`; the upstream `LICENSE.md` accompanies
the crate. Recorded in the repository's `THIRD_PARTY_NOTICES.md`.
