#!/usr/bin/env python3
"""Mint HS256 JWTs for ClusterScope auth checks (QA fixture, no network).

usage: jwt-mint.py <secret> <role> [ttl_seconds] [subject]
  ttl_seconds > 0  -> exp in the future (valid token)
  ttl_seconds < 0  -> exp in the past   (expired token)
"""
import base64
import hashlib
import hmac
import json
import sys
import time
import uuid


def b64(raw: bytes) -> str:
    return base64.urlsafe_b64encode(raw).rstrip(b"=").decode()


def main() -> int:
    secret = sys.argv[1]
    role = sys.argv[2]
    ttl = int(sys.argv[3]) if len(sys.argv) > 3 else 3600
    sub = sys.argv[4] if len(sys.argv) > 4 else "qa-user-0001"
    now = int(time.time())
    header = {"alg": "HS256", "typ": "JWT"}
    claims = {"sub": sub, "role": role, "exp": now + ttl, "iat": now, "jti": str(uuid.uuid4())}
    signing_input = b64(json.dumps(header, separators=(",", ":")).encode()) + "." + b64(
        json.dumps(claims, separators=(",", ":")).encode()
    )
    sig = hmac.new(secret.encode(), signing_input.encode(), hashlib.sha256).digest()
    print(signing_input + "." + b64(sig))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
