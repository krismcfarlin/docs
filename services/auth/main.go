// bamako-auth — lightweight auth service for Bamako sqld
//
// Validates Google access tokens, checks an email allowlist,
// and issues Ed25519-signed JWTs that sqld accepts.
//
// Endpoints:
//   GET  /auth/health
//   POST /auth/token          body: {access_token: "..."}  → {sqld_token, email, expires_in}
//   GET  /auth/invites        admin only → [{email, added_at, added_by}]
//   POST /auth/invites        admin only, body: {email, added_by}
//   DELETE /auth/invites/:email  admin only

package main

import (
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net/http"
	"os"
	"strings"
	"sync"
	"time"
)

// ── state ─────────────────────────────────────────────────────────────────────

var (
	privateKey    ed25519.PrivateKey
	adminToken    string
	allowlistPath string
	mu            sync.RWMutex
	allowlist     = map[string]AllowlistEntry{}
)

type AllowlistEntry struct {
	Email   string `json:"email"`
	AddedAt int64  `json:"added_at"`
	AddedBy string `json:"added_by,omitempty"`
}

// ── main ──────────────────────────────────────────────────────────────────────

func main() {
	privKeyB64 := mustEnv("JWT_PRIVATE_KEY")
	adminToken = mustEnv("ADMIN_TOKEN")
	allowlistPath = getEnv("ALLOWLIST_PATH", "/opt/bamako/allowlist.json")
	port := getEnv("PORT", "8091")

	seed, err := base64.RawURLEncoding.DecodeString(privKeyB64)
	if err != nil || len(seed) != 32 {
		log.Fatalf("JWT_PRIVATE_KEY must be 32-byte base64url-encoded seed (got err=%v, len=%d)", err, len(seed))
	}
	privateKey = ed25519.NewKeyFromSeed(seed)

	loadAllowlist()

	mux := http.NewServeMux()
	mux.HandleFunc("/auth/health", handleHealth)
	mux.HandleFunc("/auth/token", handleToken)
	mux.HandleFunc("/auth/admin", handleAdminToken)
	mux.HandleFunc("/auth/invites", requireAdmin(handleInvites))
	mux.HandleFunc("/auth/invites/", requireAdmin(handleInviteByEmail))

	log.Printf("bamako-auth listening on :%s", port)
	log.Fatal(http.ListenAndServe(":"+port, mux))
}

// ── handlers ──────────────────────────────────────────────────────────────────

func handleHealth(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{"status": "ok"})
}

func handleToken(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	var req struct {
		AccessToken string `json:"access_token"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil || req.AccessToken == "" {
		http.Error(w, "missing access_token", http.StatusBadRequest)
		return
	}

	email, err := googleEmailFromAccessToken(req.AccessToken)
	if err != nil {
		http.Error(w, "invalid google token: "+err.Error(), http.StatusUnauthorized)
		return
	}

	mu.RLock()
	_, ok := allowlist[strings.ToLower(email)]
	mu.RUnlock()
	if !ok {
		http.Error(w, "email not in allowlist", http.StatusForbidden)
		return
	}

	tok, err := signJWT(privateKey, map[string]any{
		"sub": email,
		"iat": time.Now().Unix(),
		"exp": time.Now().Add(24 * time.Hour).Unix(),
	})
	if err != nil {
		http.Error(w, "signing error", http.StatusInternalServerError)
		return
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]any{
		"sqld_token": tok,
		"email":      email,
		"expires_in": 86400,
	})
}

// handleAdminToken issues a JWT when the caller proves they hold the admin token.
// Used by owners who don't have a fresh Google access token.
func handleAdminToken(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	var req struct {
		AdminToken string `json:"admin_token"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil || req.AdminToken == "" {
		http.Error(w, "missing admin_token", http.StatusBadRequest)
		return
	}
	if req.AdminToken != adminToken {
		http.Error(w, "forbidden", http.StatusForbidden)
		return
	}
	tok, err := signJWT(privateKey, map[string]any{
		"sub": "admin",
		"iat": time.Now().Unix(),
		"exp": time.Now().Add(24 * time.Hour).Unix(),
	})
	if err != nil {
		http.Error(w, "signing error", http.StatusInternalServerError)
		return
	}
	log.Printf("/auth/admin: issued admin JWT")
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]any{
		"sqld_token": tok,
		"expires_in": 86400,
	})
}

// handleInvites handles GET (list) and POST (add) on /auth/invites
func handleInvites(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		mu.RLock()
		entries := make([]AllowlistEntry, 0, len(allowlist))
		for _, e := range allowlist {
			entries = append(entries, e)
		}
		mu.RUnlock()
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(entries)

	case http.MethodPost:
		var req struct {
			Email   string `json:"email"`
			AddedBy string `json:"added_by"`
		}
		if err := json.NewDecoder(r.Body).Decode(&req); err != nil || req.Email == "" {
			http.Error(w, "missing email", http.StatusBadRequest)
			return
		}
		email := strings.ToLower(strings.TrimSpace(req.Email))
		mu.Lock()
		allowlist[email] = AllowlistEntry{Email: email, AddedAt: time.Now().Unix(), AddedBy: req.AddedBy}
		saveAllowlist()
		mu.Unlock()
		w.WriteHeader(http.StatusNoContent)

	default:
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
	}
}

// handleInviteByEmail handles DELETE /auth/invites/:email
func handleInviteByEmail(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodDelete {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	email := strings.ToLower(strings.TrimPrefix(r.URL.Path, "/auth/invites/"))
	if email == "" {
		http.Error(w, "missing email", http.StatusBadRequest)
		return
	}
	mu.Lock()
	delete(allowlist, email)
	saveAllowlist()
	mu.Unlock()
	w.WriteHeader(http.StatusNoContent)
}

// ── middleware ────────────────────────────────────────────────────────────────

func requireAdmin(next http.HandlerFunc) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		tok := strings.TrimPrefix(r.Header.Get("Authorization"), "Bearer ")
		if tok == "" || tok != adminToken {
			http.Error(w, "forbidden", http.StatusForbidden)
			return
		}
		next(w, r)
	}
}

// ── allowlist ─────────────────────────────────────────────────────────────────

func loadAllowlist() {
	mu.Lock()
	defer mu.Unlock()
	data, err := os.ReadFile(allowlistPath)
	if err != nil {
		return
	}
	var entries []AllowlistEntry
	if err := json.Unmarshal(data, &entries); err != nil {
		log.Printf("warn: failed to parse allowlist %s: %v", allowlistPath, err)
		return
	}
	for _, e := range entries {
		allowlist[strings.ToLower(e.Email)] = e
	}
	log.Printf("loaded %d allowed emails from %s", len(allowlist), allowlistPath)
}

func saveAllowlist() { // call with mu held (write lock)
	entries := make([]AllowlistEntry, 0, len(allowlist))
	for _, e := range allowlist {
		entries = append(entries, e)
	}
	data, _ := json.MarshalIndent(entries, "", "  ")
	if err := os.WriteFile(allowlistPath, data, 0600); err != nil {
		log.Printf("warn: failed to save allowlist: %v", err)
	}
}

// ── Google validation ─────────────────────────────────────────────────────────

func googleEmailFromAccessToken(accessToken string) (string, error) {
	resp, err := http.Get("https://www.googleapis.com/oauth2/v3/userinfo?access_token=" + accessToken)
	if err != nil {
		return "", fmt.Errorf("userinfo request failed: %w", err)
	}
	defer resp.Body.Close()
	body, _ := io.ReadAll(resp.Body)
	var info struct {
		Email         string `json:"email"`
		EmailVerified bool   `json:"email_verified"`
		Error         string `json:"error"`
	}
	json.Unmarshal(body, &info)
	if resp.StatusCode != http.StatusOK || info.Email == "" {
		return "", fmt.Errorf("google returned %d: %s", resp.StatusCode, info.Error)
	}
	if !info.EmailVerified {
		return "", fmt.Errorf("email not verified")
	}
	return info.Email, nil
}

// ── JWT ───────────────────────────────────────────────────────────────────────

func signJWT(key ed25519.PrivateKey, claims map[string]any) (string, error) {
	b64 := base64.RawURLEncoding.EncodeToString
	header := b64([]byte(`{"alg":"EdDSA","typ":"JWT"}`))
	payloadBytes, err := json.Marshal(claims)
	if err != nil {
		return "", err
	}
	payload := b64(payloadBytes)
	msg := header + "." + payload
	sig := ed25519.Sign(key, []byte(msg))
	return msg + "." + b64(sig), nil
}

// ── helpers ───────────────────────────────────────────────────────────────────

func mustEnv(k string) string {
	v := os.Getenv(k)
	if v == "" {
		log.Fatalf("required env var %s is not set", k)
	}
	return v
}

func getEnv(k, fallback string) string {
	if v := os.Getenv(k); v != "" {
		return v
	}
	return fallback
}
