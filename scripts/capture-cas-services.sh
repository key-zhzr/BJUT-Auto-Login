#!/usr/bin/env bash

# Capture the BJUT CAS -> UC and CAS -> mobile-portal flows for offline analysis.
#
# Safety properties:
# - the password is read without echo and never written to disk or argv;
# - exactly one credential-bearing CAS POST is sent;
# - every later request is a read-only GET and every redirect is host-checked;
# - password changes, recharges and other state-changing APIs are never called;
# - cookies are held in a temporary file and removed on every exit path;
# - only the automatically redacted *.share archive is intended to be shared.

set -Eeuo pipefail
umask 077

readonly CAS_HOST="cas.bjut.edu.cn"
readonly UC_HOST="uc.bjut.edu.cn"
readonly ITS_HOST="itsapp.bjut.edu.cn"
readonly YD_HOST="ydapp.bjut.edu.cn"
readonly UC_LOGIN_ENTRY_URL="https://${UC_HOST}/api/login?target=https%3A%2F%2F${UC_HOST}%2F%23%2Fuser%2Flogin"
readonly YD_ENTRY_URL="https://${YD_HOST}/openV8HomePage"
readonly WECHAT_UA="Mozilla/5.0 (Linux; Android 13; M2102K1C Build/TKQ1.220829.002; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/116.0.0.0 Mobile Safari/537.36 XWEB/1160065 MMWEBSDK/20231202 MicroMessenger/8.0.47.2560(0x28002F30) WeChat/arm64 Weixin NetType/WIFI Language/zh_CN ABI/arm64"
readonly META_FORMAT=$'http_code=%{http_code}\nurl_effective=%{url_effective}\nremote_ip=%{remote_ip}\ncontent_type=%{content_type}\nnum_redirects=%{num_redirects}\nssl_verify_result=%{ssl_verify_result}\ntime_total=%{time_total}\n'

SCRIPT_DIR="$(CDPATH= cd "$(dirname "$0")" && pwd)"
REPO_DIR="$(CDPATH= cd "${SCRIPT_DIR}/.." && pwd)"
HELPER="${SCRIPT_DIR}/cas_services_capture.py"
CAPTURE_ROOT="${REPO_DIR}/billing-capture.local"
ALLOW_INSECURE=0
ACCOUNT=""
PASSWORD=""
FORM_BODY=""
COOKIE_FILE=""
WORK_DIR=""

usage() {
  cat <<'EOF'
Usage: ./scripts/capture-cas-services.sh [options]

Options:
  --output DIR  Store captures below DIR (default: billing-capture.local)
  --insecure    Disable TLS certificate verification (diagnostic use only)
  -h, --help    Show this help

The script prompts for a BJUT unified-authentication account and password. It
sends one CAS login POST, then only read-only GET requests. It does not submit a
password change, recharge, payment, account edit, or any other state change.
EOF
}

die() {
  printf 'Error: %s\n' "$*" >&2
  exit 1
}

cleanup() {
  local exit_code=$?
  PASSWORD=""
  FORM_BODY=""
  if [[ -n "${COOKIE_FILE}" && -f "${COOKIE_FILE}" ]]; then
    rm -f "${COOKIE_FILE}"
  fi
  if [[ -n "${WORK_DIR}" && -d "${WORK_DIR}" ]]; then
    rm -rf "${WORK_DIR}"
  fi
  trap - EXIT INT TERM HUP
  exit "${exit_code}"
}
trap cleanup EXIT INT TERM HUP

while (($#)); do
  case "$1" in
    --output)
      (($# >= 2)) || die "--output requires a directory"
      CAPTURE_ROOT="$2"
      shift 2
      ;;
    --insecure)
      ALLOW_INSECURE=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown option: $1"
      ;;
  esac
done

command -v curl >/dev/null 2>&1 || die "curl is required"
command -v python3 >/dev/null 2>&1 || die "python3 is required"
[[ -f "${HELPER}" ]] || die "missing helper: ${HELPER}"
[[ -t 0 ]] || die "run this script in an interactive terminal"

printf 'BJUT unified-authentication account: '
IFS= read -r ACCOUNT
[[ -n "${ACCOUNT}" ]] || die "account cannot be empty"

printf 'BJUT unified-authentication password (input hidden): '
IFS= read -r -s PASSWORD
printf '\n'
[[ -n "${PASSWORD}" ]] || die "password cannot be empty"

mkdir -p "${CAPTURE_ROOT}"
chmod 700 "${CAPTURE_ROOT}"
timestamp="$(date '+%Y%m%d-%H%M%S')"
RAW_DIR="${CAPTURE_ROOT}/cas-services-${timestamp}.private"
SHARE_DIR="${CAPTURE_ROOT}/cas-services-${timestamp}.share"
mkdir -p "${RAW_DIR}" "${SHARE_DIR}" "${RAW_DIR}/assets"
chmod 700 "${RAW_DIR}" "${SHARE_DIR}" "${RAW_DIR}/assets"

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/bjut-al-cas-services.XXXXXX")"
COOKIE_FILE="${WORK_DIR}/cookies.txt"
touch "${COOKIE_FILE}"
chmod 600 "${COOKIE_FILE}"

cat > "${RAW_DIR}/PRIVATE-DO-NOT-SHARE.txt" <<'EOF'
This directory contains raw authenticated pages and can contain personal data,
cookies in response headers, CAS tickets and a mobile-portal openid. Never send,
sync or commit it. Share only the sibling *.share.zip or *.share.tar.gz after
inspecting report.json and sanitized/.
EOF

CURL_COMMON=(
  --silent
  --show-error
  --connect-timeout 15
  --max-time 120
  --max-redirs 0
  --proto '=https'
  --proto-redir '=https'
  --compressed
  --cookie "${COOKIE_FILE}"
  --cookie-jar "${COOKIE_FILE}"
  --user-agent "${WECHAT_UA}"
  --header "Accept-Language: zh-CN,zh;q=0.9"
  --header "Cache-Control: no-cache"
  --header "Pragma: no-cache"
)
if ((ALLOW_INSECURE)); then
  CURL_COMMON+=(--insecure)
fi

meta_value() {
  local key="$1"
  local file="$2"
  sed -n "s/^${key}=//p" "${file}" | tail -n 1
}

# Follow a GET redirect chain one hop at a time. Every Location is validated by
# the helper before the next request, so an unexpected host is never contacted.
follow_get() {
  local capture_id="$1"
  local start_url="$2"
  local allowed_hosts="$3"
  local referer_url="$4"
  local extension="$5"
  local current_url="${start_url}"
  local step=0
  local status=""
  local next_url=""
  local step_headers=""
  local step_body=""
  local step_meta=""
  local request_args=(
    --header "Accept: text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8"
    --header "Sec-Fetch-Dest: document"
    --header "Sec-Fetch-Mode: navigate"
    --header "Upgrade-Insecure-Requests: 1"
  )

  : > "${RAW_DIR}/${capture_id}.headers"
  while ((step < 12)); do
    step=$((step + 1))
    step_headers="${WORK_DIR}/${capture_id}-${step}.headers"
    step_body="${WORK_DIR}/${capture_id}-${step}.body"
    step_meta="${WORK_DIR}/${capture_id}-${step}.meta"
    if [[ -n "${referer_url}" ]]; then
      request_args+=(--referer "${referer_url}")
    fi
    if ! curl "${CURL_COMMON[@]}" "${request_args[@]}" \
        --dump-header "${step_headers}" \
        --output "${step_body}" \
        --write-out "${META_FORMAT}" \
        "${current_url}" > "${step_meta}"; then
      printf 'Warning: GET failed while following %s\n' "${capture_id}" >&2
      return 1
    fi
    cat "${step_headers}" >> "${RAW_DIR}/${capture_id}.headers"
    status="$(meta_value http_code "${step_meta}")"
    case "${status}" in
      301|302|303|307|308)
        next_url="$(python3 "${HELPER}" safe-redirect \
          --base-url "${current_url}" \
          --allowed-hosts "${allowed_hosts}" \
          "${step_headers}")" || return 1
        referer_url="${current_url}"
        current_url="${next_url}"
        request_args=(
          --header "Accept: text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8"
          --header "Sec-Fetch-Dest: document"
          --header "Sec-Fetch-Mode: navigate"
          --header "Upgrade-Insecure-Requests: 1"
        )
        ;;
      *)
        if [[ "${status}" == "200" && "${current_url}" == "https://${ITS_HOST}/uc/api/oauth/index"* ]]; then
          if next_url="$(python3 "${HELPER}" itsapp-js-challenge \
              --base-url "${current_url}" \
              "${step_body}" 2>/dev/null)"; then
            cp "${step_body}" "${RAW_DIR}/${capture_id}.challenge-${step}.html"
            cp "${step_meta}" "${RAW_DIR}/${capture_id}.challenge-${step}.meta"
            referer_url="${current_url}"
            current_url="${next_url}"
            request_args=(
              --header "Accept: text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8"
              --header "Sec-Fetch-Dest: document"
              --header "Sec-Fetch-Mode: navigate"
              --header "Upgrade-Insecure-Requests: 1"
            )
            continue
          fi
        fi
        cp "${step_body}" "${RAW_DIR}/${capture_id}.${extension}"
        cp "${step_meta}" "${RAW_DIR}/${capture_id}.meta"
        printf 'redirect_hops=%s\n' "$((step - 1))" >> "${RAW_DIR}/${capture_id}.meta"
        return 0
        ;;
    esac
  done
  printf 'Warning: %s exceeded the 12-hop redirect limit\n' "${capture_id}" >&2
  return 1
}

fetch_readonly_api() {
  local capture_id="$1"
  local url="$2"
  local referer_url="$3"
  if ! curl "${CURL_COMMON[@]}" \
      --header "Accept: application/json, text/plain, */*" \
      --header "X-Requested-With: XMLHttpRequest" \
      --header "Sec-Fetch-Dest: empty" \
      --header "Sec-Fetch-Mode: cors" \
      --header "Sec-Fetch-Site: same-origin" \
      --referer "${referer_url}" \
      --dump-header "${RAW_DIR}/${capture_id}.headers" \
      --output "${RAW_DIR}/${capture_id}.json" \
      --write-out "${META_FORMAT}" \
      "${url}" > "${RAW_DIR}/${capture_id}.meta"; then
    printf 'Warning: read-only endpoint %s could not be captured\n' "${capture_id}" >&2
    return 0
  fi
  local status
  status="$(meta_value http_code "${RAW_DIR}/${capture_id}.meta")"
  case "${status}" in
    200) ;;
    *) printf 'Warning: %s returned HTTP %s\n' "${capture_id}" "${status:-unknown}" >&2 ;;
  esac
}

fetch_readonly_json_post() {
  local capture_id="$1"
  local url="$2"
  local referer_url="$3"
  if ! curl "${CURL_COMMON[@]}" \
      --header "Accept: application/json, text/plain, */*" \
      --header "Content-Type: application/json" \
      --header "X-Requested-With: XMLHttpRequest" \
      --header "session-type: uniapp" \
      --header "isWechatApp: true" \
      --header "orgid: 2" \
      --header "Origin: https://${YD_HOST}" \
      --header "Sec-Fetch-Dest: empty" \
      --header "Sec-Fetch-Mode: cors" \
      --header "Sec-Fetch-Site: same-origin" \
      --referer "${referer_url}" \
      --data-binary @- \
      --dump-header "${RAW_DIR}/${capture_id}.headers" \
      --output "${RAW_DIR}/${capture_id}.json" \
      --write-out "${META_FORMAT}" \
      "${url}" > "${RAW_DIR}/${capture_id}.meta"; then
    printf 'Warning: read-only query %s could not be captured\n' "${capture_id}" >&2
    return 1
  fi
  local status
  status="$(meta_value http_code "${RAW_DIR}/${capture_id}.meta")"
  if [[ "${status}" != "200" ]]; then
    printf 'Warning: %s returned HTTP %s\n' "${capture_id}" "${status:-unknown}" >&2
    return 1
  fi
}

# Fetch the assets linked by an HTML document, then follow literal same-origin
# JS/CSS references for a few bounded passes. This captures lazy chunks without
# executing the application or invoking its state-changing actions.
capture_assets() {
  local scope="$1"
  local base_url="$2"
  shift 2
  local queue="${WORK_DIR}/${scope}-asset-queue.txt"
  local next_queue="${WORK_DIR}/${scope}-asset-next.txt"
  local seen="${WORK_DIR}/${scope}-asset-seen.txt"
  local asset_dir="${RAW_DIR}/assets/${scope}"
  local asset_url=""
  local asset_name=""
  local asset_status=""
  local asset_count=0
  local unavailable_count=0
  local pass=0

  mkdir -p "${asset_dir}"
  chmod 700 "${asset_dir}"
  python3 "${HELPER}" asset-urls --base-url "${base_url}" "$@" > "${queue}"
  : > "${seen}"
  : > "${RAW_DIR}/assets/${scope}.tsv"
  : > "${RAW_DIR}/assets/${scope}-unavailable.tsv"

  for ((pass = 1; pass <= 4; pass++)); do
    [[ -s "${queue}" ]] || break
    : > "${next_queue}"
    while IFS= read -r asset_url; do
      [[ -n "${asset_url}" ]] || continue
      if grep -Fqx "${asset_url}" "${seen}"; then
        continue
      fi
      printf '%s\n' "${asset_url}" >> "${seen}"
      if ((asset_count >= 160)); then
        printf 'Warning: %s asset capture reached the 160-file safety limit\n' "${scope}" >&2
        break 2
      fi
      asset_count=$((asset_count + 1))
      asset_name="$(python3 "${HELPER}" asset-name "${asset_count}" "${asset_url}")"
      if ! asset_status="$(curl "${CURL_COMMON[@]}" \
          --header "Accept: */*" \
          --header "Sec-Fetch-Dest: script" \
          --header "Sec-Fetch-Mode: no-cors" \
          --header "Sec-Fetch-Site: same-origin" \
          --referer "${base_url}" \
          --output "${asset_dir}/${asset_name}" \
          --write-out '%{http_code}' \
          "${asset_url}")"; then
        unavailable_count=$((unavailable_count + 1))
        printf 'network-error\t%s\n' "${asset_url}" >> "${RAW_DIR}/assets/${scope}-unavailable.tsv"
        rm -f "${asset_dir}/${asset_name}"
        continue
      fi
      if [[ "${asset_status}" != "200" ]]; then
        unavailable_count=$((unavailable_count + 1))
        printf 'HTTP-%s\t%s\n' "${asset_status}" "${asset_url}" >> "${RAW_DIR}/assets/${scope}-unavailable.tsv"
        rm -f "${asset_dir}/${asset_name}"
        continue
      fi
      printf '%s\t%s\n' "${asset_name}" "${asset_url}" >> "${RAW_DIR}/assets/${scope}.tsv"
      python3 "${HELPER}" asset-urls \
        --base-url "${asset_url}" \
        "${asset_dir}/${asset_name}" >> "${next_queue}" || true
    done < "${queue}"
    sort -u "${next_queue}" > "${queue}"
  done
  if ((unavailable_count > 0)); then
    printf 'Warning: %s skipped %s unavailable referenced asset(s); details were saved in the capture package\n' \
      "${scope}" "${unavailable_count}" >&2
  else
    rm -f "${RAW_DIR}/assets/${scope}-unavailable.tsv"
  fi
}

printf '1/7 Fetching the CAS login page...\n'
follow_get "00-cas-login" "${UC_LOGIN_ENTRY_URL}" "${CAS_HOST},${UC_HOST}" "" "html" \
  || die "could not fetch the trusted CAS login page"
CAS_LOGIN_EFFECTIVE="$(meta_value url_effective "${RAW_DIR}/00-cas-login.meta")"
case "${CAS_LOGIN_EFFECTIVE}" in
  "https://${CAS_HOST}/login"*) ;;
  *) die "CAS login page ended at an unexpected URL" ;;
esac

CAPTCHA_STATE="$(python3 "${HELPER}" captcha-state \
  --base-url "${CAS_LOGIN_EFFECTIVE}" \
  "${RAW_DIR}/00-cas-login.html")" \
  || die "could not validate the CAS login form"
[[ "${CAPTCHA_STATE}" == "clear" ]] \
  || die "CAS currently requires a visual CAPTCHA; no login request was sent"
LOGIN_ACTION="$(python3 "${HELPER}" login-action \
  --base-url "${CAS_LOGIN_EFFECTIVE}" \
  "${RAW_DIR}/00-cas-login.html")" \
  || die "could not validate the CAS login form action"

printf '2/7 Replaying and saving the CAS page assets...\n'
capture_assets "cas" "${CAS_LOGIN_EFFECTIVE}" "${RAW_DIR}/00-cas-login.html"

printf '3/7 Sending one CAS login request...\n'
# Build the form completely before curl starts, so a parser error cannot cause
# an accidental empty POST. The encoded body remains in shell memory only.
FORM_BODY="$(printf '%s' "${PASSWORD}" \
  | python3 "${HELPER}" form-body \
      --base-url "${CAS_LOGIN_EFFECTIVE}" \
      --account "${ACCOUNT}" \
      "${RAW_DIR}/00-cas-login.html")" \
  || die "could not build the validated CAS login form; no request was sent"

if ! printf '%s' "${FORM_BODY}" \
  | curl "${CURL_COMMON[@]}" \
      --header "Content-Type: application/x-www-form-urlencoded" \
      --header "Accept: text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8" \
      --header "Origin: https://${CAS_HOST}" \
      --header "Sec-Fetch-Dest: document" \
      --header "Sec-Fetch-Mode: navigate" \
      --header "Sec-Fetch-Site: same-origin" \
      --header "Sec-Fetch-User: ?1" \
      --header "Upgrade-Insecure-Requests: 1" \
      --referer "${CAS_LOGIN_EFFECTIVE}" \
      --data-binary @- \
      --dump-header "${RAW_DIR}/01-cas-submit.headers" \
      --output "${RAW_DIR}/01-cas-submit.html" \
      --write-out "${META_FORMAT}" \
      "${LOGIN_ACTION}" > "${RAW_DIR}/01-cas-submit.meta"; then
  FORM_BODY=""
  die "the single CAS login request failed; it was not retried"
fi
FORM_BODY=""

LOGIN_STATUS="$(meta_value http_code "${RAW_DIR}/01-cas-submit.meta")"
LOGIN_OUTCOME="rejected"
UC_EFFECTIVE=""
case "${LOGIN_STATUS}" in
  301|302|303)
    UC_START_URL="$(python3 "${HELPER}" safe-redirect \
      --base-url "${LOGIN_ACTION}" \
      --allowed-hosts "${CAS_HOST},${UC_HOST}" \
      "${RAW_DIR}/01-cas-submit.headers")" \
      || die "CAS redirected outside the trusted UC login flow"
    if follow_get "02-uc-home" "${UC_START_URL}" "${CAS_HOST},${UC_HOST}" \
        "${CAS_LOGIN_EFFECTIVE}" "html"; then
      UC_EFFECTIVE="$(meta_value url_effective "${RAW_DIR}/02-uc-home.meta")"
      case "${UC_EFFECTIVE}" in
        "https://${UC_HOST}"|"https://${UC_HOST}/"*) LOGIN_OUTCOME="authenticated" ;;
        *) LOGIN_OUTCOME="rejected" ;;
      esac
    fi
    ;;
  200)
    LOGIN_OUTCOME="rejected"
    ;;
  307|308)
    die "CAS requested a credential POST replay; stopped after the single safe attempt"
    ;;
  *)
    LOGIN_OUTCOME="http-${LOGIN_STATUS:-unknown}"
    ;;
esac

printf '4/7 Capturing UC read-only configuration and front-end resources...\n'
if [[ "${LOGIN_OUTCOME}" == "authenticated" ]]; then
  fetch_readonly_api "10-uc-register-rules" "https://${UC_HOST}/api/register/rules" "${UC_EFFECTIVE}"
  fetch_readonly_api "11-uc-status" "https://${UC_HOST}/api/uc/status" "${UC_EFFECTIVE}"
  fetch_readonly_api "12-uc-reset-rules" "https://${UC_HOST}/api/reset/rules" "${UC_EFFECTIVE}"
  fetch_readonly_api "13-uc-userinfo" "https://${UC_HOST}/api/uc/userinfo" "${UC_EFFECTIVE}"
  fetch_readonly_api "14-uc-common-config" "https://${UC_HOST}/api/uc/commonConfig" "${UC_EFFECTIVE}"
  capture_assets "uc" "${UC_EFFECTIVE}" "${RAW_DIR}/02-uc-home.html"
else
  printf 'Warning: CAS did not reach the authenticated UC page; UC capture was skipped\n' >&2
fi

printf '5/7 Entering the mobile portal with the existing CAS session...\n'
YD_EFFECTIVE=""
if [[ "${LOGIN_OUTCOME}" == "authenticated" ]]; then
  if follow_get "20-yd-entry" "${YD_ENTRY_URL}" "${CAS_HOST},${ITS_HOST},${YD_HOST}" \
      "${UC_EFFECTIVE}" "html"; then
    YD_EFFECTIVE="$(meta_value url_effective "${RAW_DIR}/20-yd-entry.meta")"
    case "${YD_EFFECTIVE}" in
      "https://${YD_HOST}"|"https://${YD_HOST}/"*) ;;
      *)
        printf 'Warning: the mobile portal did not finish on %s\n' "${YD_HOST}" >&2
        YD_EFFECTIVE=""
        ;;
    esac
  else
    printf 'Warning: the mobile-portal redirect chain could not be completed safely\n' >&2
  fi
fi

printf '6/7 Capturing the recharge SPA resources without submitting a recharge...\n'
if [[ -n "${YD_EFFECTIVE}" ]]; then
  if follow_get "21-yd-shell" "https://${YD_HOST}/" "${YD_HOST}" \
      "${YD_EFFECTIVE}" "html"; then
    YD_SHELL_EFFECTIVE="$(meta_value url_effective "${RAW_DIR}/21-yd-shell.meta")"
    capture_assets "ydapp" "${YD_SHELL_EFFECTIVE}" \
      "${RAW_DIR}/20-yd-entry.html" "${RAW_DIR}/21-yd-shell.html"
  else
    capture_assets "ydapp" "${YD_EFFECTIVE}" "${RAW_DIR}/20-yd-entry.html"
  fi
  OPENID_STATE="$(python3 "${HELPER}" openid-state \
    "${RAW_DIR}/20-yd-entry.headers" \
    "${RAW_DIR}/20-yd-entry.meta" \
    "${RAW_DIR}/20-yd-entry.html")"
  printf 'Mobile-portal openid: %s\n' "${OPENID_STATE%%:*}" >&2
  if [[ "${OPENID_STATE%%:*}" == "present" ]]; then
    if YD_OPEN_BODY="$(python3 "${HELPER}" yd-open-body \
        "${RAW_DIR}/20-yd-entry.headers" \
        "${RAW_DIR}/20-yd-entry.meta" \
        "${RAW_DIR}/20-yd-entry.html")"; then
      if printf '%s' "${YD_OPEN_BODY}" | fetch_readonly_json_post \
          "22-yd-open-net-pay" "https://${YD_HOST}/netpay/openNetPay" "${YD_EFFECTIVE}"; then
        if YD_BALANCE_BODY="$(python3 "${HELPER}" yd-balance-body \
            "${RAW_DIR}/22-yd-open-net-pay.json" 2>/dev/null)"; then
          printf '%s' "${YD_BALANCE_BODY}" | fetch_readonly_json_post \
            "23-yd-net-account-balance" \
            "https://${YD_HOST}/channel/queryNetAccBalance" \
            "${YD_EFFECTIVE}" || true
          YD_BALANCE_BODY=""
        else
          printf 'Warning: the read-only network-balance query could not be derived from openNetPay\n' >&2
        fi
      fi
      YD_OPEN_BODY=""
    fi
  fi
else
  printf 'Warning: mobile-portal asset capture was skipped\n' >&2
fi

printf '7/7 Building a redacted share package...\n'
printf '%s' "${PASSWORD}" \
  | python3 "${HELPER}" sanitize \
      --raw-dir "${RAW_DIR}" \
      --share-dir "${SHARE_DIR}" \
      --account "${ACCOUNT}" \
      --login-outcome "${LOGIN_OUTCOME}" \
      --password-stdin

PASSWORD=""
rm -f "${COOKIE_FILE}"
COOKIE_FILE=""

ARCHIVE_BASE="${CAPTURE_ROOT}/cas-services-${timestamp}.share"
if command -v zip >/dev/null 2>&1; then
  (
    cd "${CAPTURE_ROOT}"
    zip -q -r "${ARCHIVE_BASE}.zip" "$(basename "${SHARE_DIR}")"
  )
  ARCHIVE="${ARCHIVE_BASE}.zip"
else
  tar -C "${CAPTURE_ROOT}" -czf "${ARCHIVE_BASE}.tar.gz" "$(basename "${SHARE_DIR}")"
  ARCHIVE="${ARCHIVE_BASE}.tar.gz"
fi

printf '\nCapture complete.\n'
printf 'Private raw data (do not share): %s\n' "${RAW_DIR}"
printf 'Redacted archive to inspect and send: %s\n' "${ARCHIVE}"
printf 'The script sent one CAS login POST and only read-only queries afterward.\n'
printf 'No password change, recharge, payment, or other state-changing request was sent.\n'
if [[ "${LOGIN_OUTCOME}" != "authenticated" ]]; then
  printf 'Warning: CAS login outcome was %s; do not retry automatically.\n' "${LOGIN_OUTCOME}" >&2
fi
