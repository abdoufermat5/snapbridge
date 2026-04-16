#!/usr/bin/env bash
set -euo pipefail

repo="${SNAPBRIDGE_REPO:-abdoufermat5/snapbridge}"
github_api="${GITHUB_API_URL:-https://api.github.com}"
github_url="${GITHUB_SERVER_URL:-https://github.com}"
requested_version="${SNAPBRIDGE_VERSION:-latest}"

log() {
    printf 'snapbridge-installer: %s\n' "$*"
}

die() {
    printf 'snapbridge-installer: error: %s\n' "$*" >&2
    exit 1
}

need_command() {
    command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

github_curl() {
    if [ -n "${GITHUB_TOKEN:-}" ]; then
        curl -fsSL \
            -H "Authorization: Bearer ${GITHUB_TOKEN}" \
            -H "Accept: application/vnd.github+json" \
            "$1"
    else
        curl -fsSL \
            -H "Accept: application/vnd.github+json" \
            "$1"
    fi
}

detect_deb_arch() {
    case "$(uname -m)" in
        x86_64 | amd64)
            printf 'amd64'
            ;;
        aarch64 | arm64)
            printf 'arm64'
            ;;
        *)
            die "unsupported architecture: $(uname -m). Supported architectures: amd64, arm64"
            ;;
    esac
}

resolve_tag() {
    if [ "$requested_version" != "latest" ]; then
        case "$requested_version" in
            v*)
                printf '%s' "$requested_version"
                ;;
            *)
                printf 'v%s' "$requested_version"
                ;;
        esac
        return
    fi

    local release_json tag
    release_json="$(github_curl "${github_api}/repos/${repo}/releases/latest")"
    tag="$(
        printf '%s\n' "$release_json" |
            sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' |
            head -n1
    )"

    [ -n "$tag" ] || die "could not resolve latest release tag for ${repo}"
    printf '%s' "$tag"
}

install_package() {
    local package_path="$1"
    local sudo_cmd=()

    if [ "$(id -u)" -ne 0 ]; then
        need_command sudo
        sudo_cmd=(sudo)
    fi

    if command -v apt-get >/dev/null 2>&1; then
        "${sudo_cmd[@]}" apt-get install --yes "$package_path"
    elif command -v dpkg >/dev/null 2>&1; then
        "${sudo_cmd[@]}" dpkg -i "$package_path"
    else
        die "neither apt-get nor dpkg is available"
    fi
}

main() {
    need_command curl
    need_command grep
    need_command id
    need_command mktemp
    need_command sed
    need_command sha256sum
    need_command uname

    local deb_arch tag version asset base_url tmp_dir package_path checksum_line
    deb_arch="$(detect_deb_arch)"
    tag="$(resolve_tag)"
    version="${tag#v}"
    asset="snapbridge_${version}_${deb_arch}.deb"
    base_url="${github_url}/${repo}/releases/download/${tag}"
    tmp_dir="$(mktemp -d)"
    package_path="${tmp_dir}/${asset}"
    trap 'rm -rf "$tmp_dir"' EXIT

    log "installing ${repo} ${tag} for ${deb_arch}"
    log "downloading ${asset}"
    curl -fsSL "${base_url}/${asset}" -o "$package_path"
    curl -fsSL "${base_url}/SHA256SUMS" -o "${tmp_dir}/SHA256SUMS"

    checksum_line="$(grep -F " ${asset}" "${tmp_dir}/SHA256SUMS" || true)"
    [ -n "$checksum_line" ] || die "SHA256SUMS does not contain ${asset}"

    log "verifying checksum"
    printf '%s\n' "$checksum_line" | (cd "$tmp_dir" && sha256sum -c -)

    if [ "${SNAPBRIDGE_INSTALL_DRY_RUN:-0}" = "1" ]; then
        log "dry run complete; package downloaded to ${package_path}"
        return
    fi

    log "installing package"
    install_package "$package_path"
    log "installed $(snapbridge --version 2>/dev/null || printf '%s' "$tag")"
}

main "$@"
