# Sourced by release.sh and repro-check.sh: settings that make the unsigned image reproducible.
# - the git revision goes into the banner instead of "unknown"
# - source paths are rewritten so the checkout folder does not end up in panic messages
# - the PE timestamp is fixed and the random debug GUID is dropped (the linker would otherwise stamp both)
export AEON_GIT_HASH=${AEON_GIT_HASH:-$(git rev-parse --short HEAD)}
export RUSTFLAGS="--remap-path-prefix=$(pwd -W)=/aeon -C link-arg=/timestamp:0 -C link-arg=/debug:none"
