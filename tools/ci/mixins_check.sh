#!/usr/bin/env bash
# CI's horizon-mixins check (#1102): builds tools/ci/mixins-check, a native mod with a point
# injection and a helper injection, through the launcher's source route at the checked-out
# commit, so every mixins refusal and every staging failure shows on the change that causes it.
# Compile-only: no game runs and no vanilla data is read.
#   tools/ci/mixins_check.sh LAUNCHER SCRATCH
set -euo pipefail
launcher=$(realpath "$1")
scratch=$2
root=$(git rev-parse --show-toplevel)
revision=$(git -C "$root" rev-parse HEAD)

rm -rf "$scratch"
mkdir -p "$scratch"/{home,launcher,data,config,trash,clipboard,desktop}
scratch=$(realpath "$scratch")
data=$scratch/launcher

# The source the launcher would download for `revision`, from this checkout instead: a pull
# request's merge commit is in no archive GitHub serves.
source=$data/native/source/$revision
mkdir -p "$source"
git -C "$root" archive HEAD | tar -x -C "$source"
# The launcher's own download leaves links out (horizon-mixins refuses them).
find "$source" -type l -delete
printf '%s' "$revision" > "$source/.horizon-revision"

# An installed, prepared version for the build to name, recorded as a native build of `revision`
# so the launcher takes the commit from it instead of asking GitHub about it. The server export
# reads none of its game files, so its prepared resources are an empty manifest.
version=$data/versions/native-ci
mkdir -p "$version/.local/horizon-client/26.3"
touch "$version/.installed"
echo '{}' > "$version/.local/horizon-client/26.3/horizon-resources.json"
printf '{"base": "ci", "horizon": "%s", "target": "server"}\n' "$revision" > "$version/native-mods.json"

mod=$scratch/mixins_check
cp -r "$root/tools/ci/mixins-check" "$mod"
sed -i "s/^horizon = \"HEAD\"$/horizon = \"$revision\"/" "$mod/horizon-mod.toml"
sed -i "s/rev = \"HEAD\"/rev = \"$revision\"/" "$mod/Cargo.toml"

unset CARGO_TARGET_DIR
HORIZON_HOME=$scratch/home HORIZON_LAUNCHER_HOME=$data XDG_DATA_HOME=$scratch/data \
  XDG_CONFIG_HOME=$scratch/config HORIZON_LAUNCHER_NO_SELF_UPDATE=1 HORIZON_NO_AUDIO=1 \
  HORIZON_LAUNCHER_TRASH=$scratch/trash HORIZON_LAUNCHER_CLIPBOARD=$scratch/clipboard \
  HORIZON_LAUNCHER_DESKTOP=$scratch/desktop \
  "$launcher" native-build "$mod" --server-output "$scratch/engine" --version native-ci \
  --profile developer --trust-native-code
test -x "$scratch/engine/horizon-engine"
