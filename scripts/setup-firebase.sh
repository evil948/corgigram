#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROJECT_ID="corgigram-shared"
DISPLAY_NAME="Corgigram Shared"
DB_REGION="europe-west1"
DB_INSTANCE="${PROJECT_ID}-default-rtdb"
DEFAULT_URL="https://${DB_INSTANCE}.${DB_REGION}.firebasedatabase.app"

cd "$ROOT"

echo "==> Corgigram Firebase setup"
echo "    Project:  $PROJECT_ID"
echo "    Database: $DEFAULT_URL"
echo

if ! command -v npx >/dev/null 2>&1; then
  echo "ERROR: npx not found. Install Node.js first."
  exit 1
fi

FB="npx --yes firebase-tools@latest"

if ! $FB login:list 2>/dev/null | grep -q '@'; then
  echo "==> Sign in to Google (browser will open)"
  $FB login
fi

echo "==> Checking Firebase project..."
if ! $FB projects:list --json 2>/dev/null | grep -q "\"${PROJECT_ID}\""; then
  echo "==> Creating project ${PROJECT_ID}..."
  $FB projects:create "$PROJECT_ID" --display-name "$DISPLAY_NAME"
else
  echo "    Project already exists."
fi

echo "==> Enabling Realtime Database API..."
$FB projects:addfirebase "$PROJECT_ID" 2>/dev/null || true

echo "==> Creating Realtime Database instance (${DB_REGION})..."
if $FB database:instances:list --project "$PROJECT_ID" --json 2>/dev/null | grep -q "$DB_INSTANCE"; then
  echo "    Database instance already exists."
else
  $FB database:instances:create "$DB_INSTANCE" \
    --project "$PROJECT_ID" \
    --location "$DB_REGION" || {
      echo
      echo "WARN: Could not create database via CLI."
      echo "      Create it manually in Firebase Console:"
      echo "      https://console.firebase.google.com/project/${PROJECT_ID}/database"
      echo "      → Create Database → ${DB_REGION} → Start in test mode (rules will be deployed next)"
      echo
      read -r -p "Press Enter after creating the database in Console..."
    }
fi

echo "==> Deploying security rules..."
$FB deploy --only database --project "$PROJECT_ID"

echo
echo "==> Testing connection..."
HTTP_CODE="$(curl -s -o /tmp/corgigram-fb-test.json -w '%{http_code}' "${DEFAULT_URL}/.json" || true)"
if [[ "$HTTP_CODE" == "200" || "$HTTP_CODE" == "401" || "$HTTP_CODE" == "404" ]]; then
  echo "    OK (HTTP ${HTTP_CODE}) — database responds."
else
  cat /tmp/corgigram-fb-test.json 2>/dev/null || true
  echo "    WARN: unexpected HTTP ${HTTP_CODE}. Check Console."
fi

echo
echo "Done. Default URL in Corgigram:"
echo "  ${DEFAULT_URL}"
