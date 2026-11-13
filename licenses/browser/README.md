# Browser dependency licenses

The build collects license and notice files from installed production packages
into `dist/third-party-licenses.txt`. It fails if an installed package has no
license or notice file, so dependency upgrades can be reviewed deliberately.

The npm distribution of `@bufbuild/protobuf` 1.10.1 omits its license files.
The checked-in fallback files come from the exact
[upstream release](https://github.com/bufbuild/protobuf-es/tree/v1.10.1):
its Apache license and the Google BSD notice in
`packages/protobuf/src/google/varint.ts`.
