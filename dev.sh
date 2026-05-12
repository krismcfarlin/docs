#!/usr/bin/env bash
set -e

echo "[dev] Stopping previous instances..."
pkill -f "tauri dev" 2>/dev/null || true
pkill -f "npm run tauri" 2>/dev/null || true
pkill -f "bamako" 2>/dev/null || true
lsof -ti :5274 | xargs kill -9 2>/dev/null || true
sleep 1

echo "[dev] Starting..."
npm run tauri dev
