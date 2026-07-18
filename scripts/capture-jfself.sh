#!/usr/bin/env bash

# Capture the BJUT DR.COM self-service login flow for offline analysis.
#
# Security properties:
# - credentials are never embedded in this repository;
# - the password is read without echo and streamed to curl through stdin;
# - the cookie jar is temporary and deleted on every exit path;
# - raw responses stay under billing-capture.local/ (gitignored);
# - only the generated *.share.zip / *.share.tar.gz is intended to be shared.

set -Eeuo pipefail
umask 077

readonly HOST="jfself.bjut.edu.cn"
readonly FIXED_IP="172.21.0.16"
readonly LOGIN_URL="https://${HOST}/Self/login/?302=LI"
readonly VERIFY_URL="https://${HOST}/Self/login/verify"
readonly UA="Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36"

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPO_DIR="$(CDPATH= cd -- "${SCRIPT_DIR}/.." && pwd)"
HELPER="${SCRIPT_DIR}/jfself_capture.py"
CAPTURE_ROOT="${REPO_DIR}/billing-capture.local"
ROUTE_MODE="auto"
ALLOW_INSECURE=0
ACCOUNT=""
PASSWORD=""
COOKIE_FILE=""
WORK_DIR=""

usage() {
  cat <<'EOF'
Usage: ./scripts/capture-jfself.sh [options]

Options:
  --route auto|system|fixed  DNS route (default: auto; system first, then 172.21.0.16)
  --insecure                 Disable TLS certificate verification (only if campus TLS requires it)
  --output DIR               Store captures below DIR (default: billing-capture.local)
  -h, --help                 Show this help

The script prompts for the account and password. It sends exactly one login POST
per run, never changes billing/account settings, and never stores the password.
EOF
}

die() {
  printf 'Error: %s\n' "$*" >&2
  exit 1
}

cleanup() {
  local exit_code=$?
  PASSWORD=""
  if [[ -n "${COOKIE_FILE}" && -f "${COOKIE_FILE}" ]]; then
    rm -f "${COOKIE_FILE}"
  fi
  if [[ -n "${WORK_DIR}" && -d "${WORK_DIR}" ]]; then
    rm -rf "${WORK_DIR}"
  fi
  exit "${exit_code}"
}
trap cleanup EXIT INT TERM HUP

while (($#)); do
  case "$1" in
    --route)
      (($# >= 2)) || die "--route requires a value"
      ROUTE_MODE="$2"
      shift 2
      ;;
    --insecure)
      ALLOW_INSECURE=1
      shift
      ;;
    --output)
      (($# >= 2)) || die "--output requires a directory"
      CAPTURE_ROOT="$2"
      shift 2
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

case "${ROUTE_MODE}" in
  auto|system|fixed) ;;
  *) die "--route must be auto, system, or fixed" ;;
esac

command -v curl >/dev/null 2>&1 || die "curl is required"
command -v python3 >/dev/null 2>&1 || die "python3 is required"
[[ -f "${HELPER}" ]] || die "missing helper: ${HELPER}"
[[ -t 0 ]] || die "run this script in an interactive terminal so the password can be read safely"

printf 'BJUT billing account: '
IFS= read -r ACCOUNT
[[ -n "${ACCOUNT}" ]] || die "account cannot be empty"

printf 'BJUT billing password (input hidden): '
IFS= read -r -s PASSWORD
printf '\n'
[[ -n "${PASSWORD}" ]] || die "password cannot be empty"

mkdir -p "${CAPTURE_ROOT}"
chmod 700 "${CAPTURE_ROOT}"
timestamp="$(date '+%Y%m%d-%H%M%S')"
RAW_DIR="${CAPTURE_ROOT}/jfself-${timestamp}.private"
SHARE_DIR="${CAPTURE_ROOT}/jfself-${timestamp}.share"
mkdir -p "${RAW_DIR}" "${SHARE_DIR}"
chmod 700 "${RAW_DIR}" "${SHARE_DIR}"

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/bjut-al-jfself.XXXXXX")"
COOKIE_FILE="${WORK_DIR}/cookies.txt"
touch "${COOKIE_FILE}"
chmod 600 "${COOKIE_FILE}"

cat > "${RAW_DIR}/PRIVATE-DO-NOT-SHARE.txt" <<'EOF'
This directory can contain private billing details and raw authenticated HTML.
Do not commit or share it. Share only the sibling archive ending in .share.zip
or .share.tar.gz after inspecting its contents.
EOF

CURL_COMMON=(
  --silent
  --show-error
  --connect-timeout 12
  --max-time 90
  --max-redirs 5
  --proto '=https'
  --proto-redir '=https'
  --compressed
  --cookie "${COOKIE_FILE}"
  --cookie-jar "${COOKIE_FILE}"
  --user-agent "${UA}"
  --header "Accept-Language: zh-CN,zh;q=0.9,en;q=0.6"
)
if ((ALLOW_INSECURE)); then
  CURL_COMMON+=(--insecure)
fi

route_args=()
ACTIVE_ROUTE="${ROUTE_MODE}"
if [[ "${ROUTE_MODE}" == "fixed" ]]; then
  route_args=(--resolve "${HOST}:443:${FIXED_IP}")
fi

curl_get_login() {
  local header_file="$1"
  local body_file="$2"
  local meta_file="$3"
  shift 3
  curl "${CURL_COMMON[@]}" --location "$@" \
    --header "Accept: text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8" \
    --header "Sec-Fetch-Dest: document" \
    --header "Sec-Fetch-Mode: navigate" \
    --header "Sec-Fetch-Site: none" \
    --header "Upgrade-Insecure-Requests: 1" \
    --dump-header "${header_file}" \
    --output "${body_file}" \
    --write-out $'http_code=%{http_code}\nurl_effective=%{url_effective}\nremote_ip=%{remote_ip}\ncontent_type=%{content_type}\nnum_redirects=%{num_redirects}\nssl_verify_result=%{ssl_verify_result}\ntime_total=%{time_total}\n' \
    "${LOGIN_URL}" > "${meta_file}"
}

printf '1/6 Fetching login page...\n'
if [[ "${ROUTE_MODE}" == "auto" ]]; then
  if curl_get_login "${RAW_DIR}/00-login.headers" "${RAW_DIR}/00-login.html" "${RAW_DIR}/00-login.meta"; then
    ACTIVE_ROUTE="system"
  else
    printf 'System DNS route failed; retrying the read-only login page with %s pinned...\n' "${FIXED_IP}" >&2
    : > "${COOKIE_FILE}"
    route_args=(--resolve "${HOST}:443:${FIXED_IP}")
    curl_get_login "${RAW_DIR}/00-login.headers" "${RAW_DIR}/00-login.html" "${RAW_DIR}/00-login.meta" ${route_args[@]+"${route_args[@]}"} \
      || die "cannot reach ${HOST}; verify the campus network, then retry (use --insecure only for a certificate error)"
    ACTIVE_ROUTE="fixed"
  fi
else
  curl_get_login "${RAW_DIR}/00-login.headers" "${RAW_DIR}/00-login.html" "${RAW_DIR}/00-login.meta" ${route_args[@]+"${route_args[@]}"} \
    || die "cannot fetch the login page; verify the selected route and campus connection"
fi

CHECKCODE="$(python3 "${HELPER}" checkcode "${RAW_DIR}/00-login.html")" \
  || die "could not extract checkcode from the login page"
[[ "${CHECKCODE}" =~ ^[0-9]{1,4}$ ]] || die "login page returned an unexpected checkcode format (expected 1-4 digits); no login request was sent"
LOGIN_URL_EFFECTIVE="$(sed -n 's/^url_effective=//p' "${RAW_DIR}/00-login.meta" | tail -n 1)"
[[ -n "${LOGIN_URL_EFFECTIVE}" ]] || LOGIN_URL_EFFECTIVE="${LOGIN_URL}"
VERIFY_ACTION="$(python3 "${HELPER}" login-action \
  --base-url "${LOGIN_URL_EFFECTIVE}" \
  "${RAW_DIR}/00-login.html")" \
  || die "could not validate the session-bound login form action"
if [[ "$(python3 "${HELPER}" captcha-state "${RAW_DIR}/00-login.html")" == "required" ]]; then
  die "the site currently requires a visual CAPTCHA; no login request was sent (complete it in a browser or retry after the cooldown)"
fi

printf '2/6 Replaying the browser login-page requests...\n'
mkdir -p "${RAW_DIR}/assets"
python3 "${HELPER}" asset-urls \
  --base-url "${LOGIN_URL_EFFECTIVE}" \
  "${RAW_DIR}/00-login.html" > "${WORK_DIR}/prelogin-assets.txt"
while IFS= read -r asset_url; do
  [[ -n "${asset_url}" ]] || continue
  if ! curl "${CURL_COMMON[@]}" --location ${route_args[@]+"${route_args[@]}"} \
      --header "Sec-Fetch-Dest: script" \
      --header "Sec-Fetch-Mode: no-cors" \
      --header "Sec-Fetch-Site: same-origin" \
      --referer "${LOGIN_URL_EFFECTIVE}" \
      --output /dev/null \
      "${asset_url}"; then
    printf 'Warning: failed to preload asset %s\n' "${asset_url}" >&2
  fi
done < "${WORK_DIR}/prelogin-assets.txt"

CACHE_BUSTER="$(date '+%s')"
curl "${CURL_COMMON[@]}" --location ${route_args[@]+"${route_args[@]}"} \
  --header "Accept: image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8" \
  --header "Sec-Fetch-Dest: image" \
  --header "Sec-Fetch-Mode: no-cors" \
  --header "Sec-Fetch-Site: same-origin" \
  --referer "${LOGIN_URL_EFFECTIVE}" \
  --output "${RAW_DIR}/00-random-code.png" \
  "https://${HOST}/Self/login/randomCode?t=0.${CACHE_BUSTER}" \
  || die "could not reproduce the browser's randomCode request; no login request was sent"

if ! curl "${CURL_COMMON[@]}" --location ${route_args[@]+"${route_args[@]}"} \
    --header "Accept: */*" \
    --header "X-Requested-With: XMLHttpRequest" \
    --header "Sec-Fetch-Dest: empty" \
    --header "Sec-Fetch-Mode: cors" \
    --header "Sec-Fetch-Site: same-origin" \
    --referer "${LOGIN_URL_EFFECTIVE}" \
    --output "${RAW_DIR}/00-brand-info.json" \
    "https://${HOST}/Self/login/getBrandInfo?t=0.${CACHE_BUSTER}"; then
  printf 'Warning: browser brand-information request failed; continuing because it is cosmetic.\n' >&2
fi

printf '3/6 Sending one login request...\n'
# The password travels from the shell to urllib.parse and then curl exclusively
# through pipes. It is never placed in argv, a temporary file, or captured output.
# Redirects are deliberately disabled for this POST. A 307/308 response could
# replay the request body, so it is rejected; only a validated same-origin
# 301/302/303 target is subsequently fetched with a credential-free GET.
printf '%s' "${PASSWORD}" \
  | python3 "${HELPER}" form-body "${CHECKCODE}" "${ACCOUNT}" \
  | curl "${CURL_COMMON[@]}" ${route_args[@]+"${route_args[@]}"} \
      --header "Content-Type: application/x-www-form-urlencoded" \
      --header "Accept: text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8" \
      --header "Origin: https://${HOST}" \
      --header "Sec-Fetch-Dest: document" \
      --header "Sec-Fetch-Mode: navigate" \
      --header "Sec-Fetch-Site: same-origin" \
      --header "Sec-Fetch-User: ?1" \
      --header "Upgrade-Insecure-Requests: 1" \
      --referer "${LOGIN_URL_EFFECTIVE}" \
      --data-binary @- \
      --dump-header "${RAW_DIR}/01-verify-initial.headers" \
      --output "${RAW_DIR}/01-verify-initial.html" \
      --write-out $'http_code=%{http_code}\nurl_effective=%{url_effective}\nremote_ip=%{remote_ip}\ncontent_type=%{content_type}\nnum_redirects=%{num_redirects}\nssl_verify_result=%{ssl_verify_result}\ntime_total=%{time_total}\n' \
      "${VERIFY_ACTION}" > "${RAW_DIR}/01-verify-initial.meta" \
  || die "the single login request failed; no retry was attempted"

VERIFY_INITIAL_STATUS="$(sed -n 's/^http_code=//p' "${RAW_DIR}/01-verify-initial.meta" | tail -n 1)"
case "${VERIFY_INITIAL_STATUS}" in
  200)
    mv "${RAW_DIR}/01-verify-initial.headers" "${RAW_DIR}/01-verify.headers"
    mv "${RAW_DIR}/01-verify-initial.html" "${RAW_DIR}/01-verify.html"
    mv "${RAW_DIR}/01-verify-initial.meta" "${RAW_DIR}/01-verify.meta"
    ;;
  301|302|303)
    VERIFY_REDIRECT_URL="$(python3 "${HELPER}" safe-redirect \
      --base-url "${VERIFY_ACTION}" \
      "${RAW_DIR}/01-verify-initial.headers")" \
      || die "login response redirected outside the trusted jfself origin"
    curl "${CURL_COMMON[@]}" --location --max-redirs 5 ${route_args[@]+"${route_args[@]}"} \
      --header "Accept: text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8" \
      --header "Sec-Fetch-Dest: document" \
      --header "Sec-Fetch-Mode: navigate" \
      --header "Sec-Fetch-Site: same-origin" \
      --header "Upgrade-Insecure-Requests: 1" \
      --referer "${LOGIN_URL_EFFECTIVE}" \
      --dump-header "${RAW_DIR}/01-verify.headers" \
      --output "${RAW_DIR}/01-verify.html" \
      --write-out $'http_code=%{http_code}\nurl_effective=%{url_effective}\nremote_ip=%{remote_ip}\ncontent_type=%{content_type}\nnum_redirects=%{num_redirects}\nssl_verify_result=%{ssl_verify_result}\ntime_total=%{time_total}\n' \
      "${VERIFY_REDIRECT_URL}" > "${RAW_DIR}/01-verify.meta" \
      || die "could not fetch the validated login result page"
    ;;
  307|308)
    die "login endpoint requested a repeated credential POST; stopped after the single safe attempt"
    ;;
  *)
    die "login endpoint returned unexpected HTTP ${VERIFY_INITIAL_STATUS:-unknown}; no retry was attempted"
    ;;
esac

VERIFY_URL_EFFECTIVE="$(sed -n 's/^url_effective=//p' "${RAW_DIR}/01-verify.meta" | tail -n 1)"
[[ -n "${VERIFY_URL_EFFECTIVE}" ]] || VERIFY_URL_EFFECTIVE="${VERIFY_URL}"
case "${VERIFY_URL_EFFECTIVE}" in
  "https://${HOST}/Self/"*) ;;
  *) die "login result page left the trusted jfself origin" ;;
esac
LOGIN_OUTCOME="$(python3 "${HELPER}" login-outcome "${RAW_DIR}/01-verify.html")"

HTML_PAGES=("${RAW_DIR}/00-login.html" "${RAW_DIR}/01-verify.html")

fetch_authenticated_page() {
  local page_id="$1"
  local page_url="$2"
  local referer_url="$3"
  curl "${CURL_COMMON[@]}" --location ${route_args[@]+"${route_args[@]}"} \
    --header "Accept: text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8" \
    --header "Sec-Fetch-Dest: document" \
    --header "Sec-Fetch-Mode: navigate" \
    --header "Sec-Fetch-Site: same-origin" \
    --header "Upgrade-Insecure-Requests: 1" \
    --referer "${referer_url}" \
    --dump-header "${RAW_DIR}/${page_id}.headers" \
    --output "${RAW_DIR}/${page_id}.html" \
    --write-out $'http_code=%{http_code}\nurl_effective=%{url_effective}\nremote_ip=%{remote_ip}\ncontent_type=%{content_type}\nnum_redirects=%{num_redirects}\nssl_verify_result=%{ssl_verify_result}\ntime_total=%{time_total}\n' \
    "${page_url}" > "${RAW_DIR}/${page_id}.meta"

  local effective_url
  effective_url="$(sed -n 's/^url_effective=//p' "${RAW_DIR}/${page_id}.meta" | tail -n 1)"
  case "${effective_url}" in
    "https://${HOST}/Self/"*) ;;
    *) die "${page_id} redirected outside the allowed jfself origin" ;;
  esac
  case "${effective_url}" in
    *"/Self/login"*) die "the authenticated session expired while fetching ${page_id}" ;;
  esac
  HTML_PAGES+=("${RAW_DIR}/${page_id}.html")
}

fetch_readonly_data() {
  local response_id="$1"
  local response_url="$2"
  local extension="$3"
  local referer_url="${4:-${VERIFY_URL_EFFECTIVE}}"
  if [[ -n "${AJAX_CSRF_TOKEN:-}" ]]; then
    if [[ "${response_url}" == *\?* ]]; then
      response_url="${response_url}&ajaxCsrfToken=${AJAX_CSRF_TOKEN}"
    else
      response_url="${response_url}?ajaxCsrfToken=${AJAX_CSRF_TOKEN}"
    fi
  fi
  if ! curl "${CURL_COMMON[@]}" --location ${route_args[@]+"${route_args[@]}"} \
      --header "Accept: application/json, text/plain, */*" \
      --header "X-Requested-With: XMLHttpRequest" \
      --header "Sec-Fetch-Dest: empty" \
      --header "Sec-Fetch-Mode: cors" \
      --header "Sec-Fetch-Site: same-origin" \
      --referer "${referer_url}" \
      --dump-header "${RAW_DIR}/${response_id}.headers" \
      --output "${RAW_DIR}/${response_id}.${extension}" \
      --write-out $'http_code=%{http_code}\nurl_effective=%{url_effective}\nremote_ip=%{remote_ip}\ncontent_type=%{content_type}\nnum_redirects=%{num_redirects}\nssl_verify_result=%{ssl_verify_result}\ntime_total=%{time_total}\n' \
      "${response_url}" > "${RAW_DIR}/${response_id}.meta"; then
    printf 'Warning: read-only endpoint %s could not be captured.\n' "${response_id}" >&2
    return
  fi

  local effective_url
  effective_url="$(sed -n 's/^url_effective=//p' "${RAW_DIR}/${response_id}.meta" | tail -n 1)"
  case "${effective_url}" in
    "https://${HOST}/Self/"*) ;;
    *) die "${response_id} redirected outside the allowed jfself origin" ;;
  esac
  case "${effective_url}" in
    *"/Self/login"*) die "the authenticated session expired while fetching ${response_id}" ;;
  esac
}

select_ajax_token_for_page() {
  local html_file="$1"
  local referer_url="$2"
  local token_script="${WORK_DIR}/page-token.js"
  local effective_url=""
  AJAX_CSRF_TOKEN="$(python3 "${HELPER}" ajax-token "${html_file}" 2>/dev/null || true)"
  if [[ -n "${AJAX_CSRF_TOKEN}" ]]; then
    return
  fi
  if ! effective_url="$(curl "${CURL_COMMON[@]}" --location ${route_args[@]+"${route_args[@]}"} \
      --header "Accept: */*" \
      --header "Sec-Fetch-Dest: script" \
      --header "Sec-Fetch-Mode: no-cors" \
      --header "Sec-Fetch-Site: same-origin" \
      --referer "${referer_url}" \
      --output "${token_script}" \
      --write-out '%{url_effective}' \
      "https://${HOST}/Self/resources/js/shareJS.js")"; then
    return
  fi
  case "${effective_url}" in
    "https://${HOST}/Self/"*) ;;
    *) die "AJAX token script redirected outside the allowed jfself origin" ;;
  esac
  AJAX_CSRF_TOKEN="$(python3 "${HELPER}" ajax-token "${token_script}" 2>/dev/null || true)"
}

printf '4/6 Fetching authenticated module pages and read-only table data...\n'
if [[ "${LOGIN_OUTCOME}" == "authenticated" ]]; then
  fetch_authenticated_page "02-bill" "https://${HOST}/Self/bill" "${VERIFY_URL_EFFECTIVE}"
  fetch_authenticated_page "03-service" "https://${HOST}/Self/service" "https://${HOST}/Self/bill"
  fetch_authenticated_page "04-setting" "https://${HOST}/Self/setting" "https://${HOST}/Self/service"
  fetch_authenticated_page "10-bill-online" "https://${HOST}/Self/bill/userOnlineLog" "https://${HOST}/Self/bill"
  fetch_authenticated_page "11-bill-month" "https://${HOST}/Self/bill/monthPay" "https://${HOST}/Self/bill"
  fetch_authenticated_page "12-bill-payment" "https://${HOST}/Self/bill/payMent" "https://${HOST}/Self/bill"
  fetch_authenticated_page "13-bill-operations" "https://${HOST}/Self/bill/operatorLog" "https://${HOST}/Self/bill"
  fetch_authenticated_page "20-service-stop" "https://${HOST}/Self/service/goStop" "https://${HOST}/Self/service"
  fetch_authenticated_page "21-service-reopen" "https://${HOST}/Self/service/goReopen" "https://${HOST}/Self/service"
  fetch_authenticated_page "22-service-reopen-history" "https://${HOST}/Self/service/goReopen?tab=1" "https://${HOST}/Self/service/goReopen"
  fetch_authenticated_page "23-service-package" "https://${HOST}/Self/service/package" "https://${HOST}/Self/service"
  fetch_authenticated_page "24-service-consume-protect" "https://${HOST}/Self/service/consumeProtect" "https://${HOST}/Self/service"
  fetch_authenticated_page "25-service-mac" "https://${HOST}/Self/service/myMac" "https://${HOST}/Self/service"
  fetch_authenticated_page "26-service-groups" "https://${HOST}/Self/service/userGroups" "https://${HOST}/Self/service"
  fetch_authenticated_page "27-service-recharge" "https://${HOST}/Self/service/userRecharge" "https://${HOST}/Self/service"
  fetch_authenticated_page "30-setting-password" "https://${HOST}/Self/setting/changePassword" "https://${HOST}/Self/setting"
  fetch_authenticated_page "31-setting-questions" "https://${HOST}/Self/setting/passwordQuestion" "https://${HOST}/Self/setting"

  IFS=$'\t' read -r QUERY_START_DATE QUERY_END_DATE QUERY_YEAR < <(
    python3 "${HELPER}" date-window --days 60
  )
  select_ajax_token_for_page "${RAW_DIR}/01-verify.html" "${VERIFY_URL_EFFECTIVE}"
  fetch_readonly_data "40-login-history" "https://${HOST}/Self/dashboard/getLoginHistory" "json"
  fetch_readonly_data "41-online-list" "https://${HOST}/Self/dashboard/getOnlineList" "json"
  fetch_readonly_data "42-offline-tip" "https://${HOST}/Self/dashboard/getOfflineTip" "txt"
  fetch_readonly_data "43-mauth-state" "https://${HOST}/Self/dashboard/refreshMauthType?t=0.${CACHE_BUSTER}" "json"
  select_ajax_token_for_page "${RAW_DIR}/10-bill-online.html" "https://${HOST}/Self/bill/userOnlineLog"
  fetch_readonly_data "50-bill-online" "https://${HOST}/Self/bill/getUserOnlineLog?pageSize=25&pageNumber=1&searchText=&sortName=loginTime&sortOrder=DESC&startTime=${QUERY_START_DATE}&endTime=${QUERY_END_DATE}" "json" "https://${HOST}/Self/bill/userOnlineLog"
  select_ajax_token_for_page "${RAW_DIR}/11-bill-month.html" "https://${HOST}/Self/bill/monthPay"
  fetch_readonly_data "51-bill-month" "https://${HOST}/Self/bill/getMonthPay?pageSize=25&pageNumber=1&searchText=&sortName=0&sortOrder=DESC&year=${QUERY_YEAR}" "json" "https://${HOST}/Self/bill/monthPay"
  select_ajax_token_for_page "${RAW_DIR}/12-bill-payment.html" "https://${HOST}/Self/bill/payMent"
  fetch_readonly_data "52-bill-payment" "https://${HOST}/Self/bill/getPayMent?pageSize=25&pageNumber=1&searchText=&sortName=0&sortOrder=DESC&startTime=${QUERY_START_DATE}&endTime=${QUERY_END_DATE}" "json" "https://${HOST}/Self/bill/payMent"
  select_ajax_token_for_page "${RAW_DIR}/13-bill-operations.html" "https://${HOST}/Self/bill/operatorLog"
  fetch_readonly_data "53-bill-operations" "https://${HOST}/Self/bill/getOperatorLog?pageSize=25&pageNumber=1&searchText=&sortName=0&sortOrder=DESC&startTime=${QUERY_START_DATE}&endTime=${QUERY_END_DATE}" "json" "https://${HOST}/Self/bill/operatorLog"
  select_ajax_token_for_page "${RAW_DIR}/20-service-stop.html" "https://${HOST}/Self/service/goStop"
  fetch_readonly_data "60-service-stop-log" "https://${HOST}/Self/service/getStopLog?pageSize=25&pageNumber=1&searchText=&sortName=&sortOrder=DESC" "json" "https://${HOST}/Self/service/goStop"
  select_ajax_token_for_page "${RAW_DIR}/21-service-reopen.html" "https://${HOST}/Self/service/goReopen"
  fetch_readonly_data "61-service-reopen-log" "https://${HOST}/Self/service/goReopenLog?pageSize=25&pageNumber=1&searchText=&sortName=&sortOrder=DESC" "json" "https://${HOST}/Self/service/goReopen"
  select_ajax_token_for_page "${RAW_DIR}/23-service-package.html" "https://${HOST}/Self/service/package"
  fetch_readonly_data "62-service-package-log" "https://${HOST}/Self/service/packageLog?pageSize=25&pageNumber=1&searchText=&sortName=fldchangedate&sortOrder=DESC" "json" "https://${HOST}/Self/service/package"
  select_ajax_token_for_page "${RAW_DIR}/25-service-mac.html" "https://${HOST}/Self/service/myMac"
  fetch_readonly_data "63-service-mac-list" "https://${HOST}/Self/service/getMacList?pageSize=25&pageNumber=1&searchText=&sortName=2&sortOrder=DESC" "json" "https://${HOST}/Self/service/myMac"
  select_ajax_token_for_page "${RAW_DIR}/26-service-groups.html" "https://${HOST}/Self/service/userGroups"
  fetch_readonly_data "64-service-groups" "https://${HOST}/Self/service/getUserGroups?pageSize=100&pageNumber=1&sortOrder=asc" "json" "https://${HOST}/Self/service/userGroups"
else
  printf 'Login was rejected; skipping authenticated module pages.\n' >&2
fi

printf '5/6 Fetching same-origin JavaScript and CSS for endpoint discovery...\n'
{
  for html_page in "${HTML_PAGES[@]}"; do
    page_meta="${html_page%.html}.meta"
    page_base="$(sed -n 's/^url_effective=//p' "${page_meta}" | tail -n 1)"
    [[ -n "${page_base}" ]] || page_base="${VERIFY_URL_EFFECTIVE}"
    python3 "${HELPER}" asset-urls --base-url "${page_base}" "${html_page}"
  done
} | sort -u > "${WORK_DIR}/asset-urls.txt"

asset_index=0
while IFS= read -r asset_url; do
  [[ -n "${asset_url}" ]] || continue
  asset_index=$((asset_index + 1))
  asset_name="$(python3 "${HELPER}" asset-name "${asset_index}" "${asset_url}")"
  if ! curl "${CURL_COMMON[@]}" --location ${route_args[@]+"${route_args[@]}"} \
      --referer "${VERIFY_URL_EFFECTIVE}" \
      --output "${RAW_DIR}/assets/${asset_name}" \
      "${asset_url}"; then
    printf 'Warning: failed to fetch asset %s\n' "${asset_url}" >&2
    rm -f "${RAW_DIR}/assets/${asset_name}"
  else
    printf '%s\t%s\n' "${asset_name}" "${asset_url}" >> "${RAW_DIR}/assets.tsv"
  fi
done < "${WORK_DIR}/asset-urls.txt"

if [[ "${LOGIN_OUTCOME}" == "authenticated" ]]; then
  if ! curl "${CURL_COMMON[@]}" --location ${route_args[@]+"${route_args[@]}"} \
      --header "Accept: text/html,*/*;q=0.8" \
      --referer "${VERIFY_URL_EFFECTIVE}" \
      --output /dev/null \
      "https://${HOST}/Self/login/logout"; then
    printf 'Warning: the temporary billing session could not be logged out cleanly.\n' >&2
  fi
fi

printf '6/6 Building a redacted share package...\n'
# Password is supplied on stdin solely so an echoed password can be removed from
# a malformed/error response. The helper never writes or prints it.
printf '%s' "${PASSWORD}" \
  | python3 "${HELPER}" sanitize \
      --raw-dir "${RAW_DIR}" \
      --share-dir "${SHARE_DIR}" \
      --account "${ACCOUNT}" \
      --checkcode "${CHECKCODE}" \
      --route "${ACTIVE_ROUTE}" \
      --password-stdin

PASSWORD=""
rm -f "${COOKIE_FILE}"
COOKIE_FILE=""

ARCHIVE_BASE="${CAPTURE_ROOT}/jfself-${timestamp}.share"
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
printf 'The script sent one login POST, read-only module GETs, and no account-changing requests.\n'
if [[ "${LOGIN_OUTCOME}" == "rejected" ]]; then
  printf 'Warning: the server returned the login form again; do not retry automatically.\n' >&2
else
  printf 'The dashboard, all known subpages, and nine read-only table endpoints were captured.\n'
fi
