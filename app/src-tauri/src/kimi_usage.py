"""Kimi's plan limits for Shifu's usage footer, run with the Python of Kimi's own install (limits.rs).

Kimi keeps the numbers on its server only. Its login token lasts minutes and the refresh token rotates on every
refresh, so the refresh goes through Kimi's own code, under its own lock, exactly as Kimi does before its /usage
command; the token never leaves this process. Reads the same fields as that command (kimi_cli/ui/shell/usage.py).

Prints {"windows": [{"used_percent", "window_minutes", "resets_at"}]} or {"error": "..."} on stdout.
"""

import asyncio
import json
from datetime import datetime

from kimi_cli.auth import KIMI_CODE_PLATFORM_ID
from kimi_cli.auth.oauth import OAuthManager
from kimi_cli.auth.platforms import get_platform_by_id, managed_provider_key
from kimi_cli.config import load_config
from kimi_cli.utils.aiohttp import new_client_session

UNIT_MINUTES = {"TIME_UNIT_MINUTE": 1, "TIME_UNIT_HOUR": 60, "TIME_UNIT_DAY": 1440}
WEEK = 7 * 1440


def window(detail, minutes):
    limit = int(detail.get("limit") or 0)
    used = detail.get("used")
    used = int(used) if used is not None else limit - int(detail.get("remaining") or limit)
    reset = detail.get("resetTime") or detail.get("reset_time")
    if limit <= 0 or not reset:
        return None
    resets_at = int(datetime.fromisoformat(reset.replace("Z", "+00:00")).timestamp())
    return {"used_percent": round(used * 100 / limit, 2), "window_minutes": minutes, "resets_at": resets_at}


def windows(payload):
    """The short windows from "limits", then the weekly one from "usage", as Kimi's /usage lists them."""
    found = []
    for item in payload.get("limits") or []:
        span = item.get("window") or {}
        minutes = int(span.get("duration") or 0) * UNIT_MINUTES.get(span.get("timeUnit"), 0)
        w = window(item.get("detail") or item, minutes) if minutes else None
        if w:
            found.append(w)
    if isinstance(payload.get("usage"), dict):
        w = window(payload["usage"], WEEK)
        if w:
            found.append(w)
    return found


async def main():
    config = load_config()
    provider = config.providers.get(managed_provider_key(KIMI_CODE_PLATFORM_ID))
    if provider is None or provider.oauth is None:
        return {"error": "没登录 Kimi Code"}
    oauth = OAuthManager(config)
    await oauth.ensure_fresh(None)
    key = oauth.resolve_api_key(provider.api_key, provider.oauth)
    url = get_platform_by_id(KIMI_CODE_PLATFORM_ID).base_url.rstrip("/") + "/usages"
    async with new_client_session() as session, session.get(url, headers={"Authorization": f"Bearer {key}"}) as resp:
        if resp.status != 200:
            return {"error": f"查用量返回 {resp.status}"}
        return {"windows": windows(await resp.json())}


print(json.dumps(asyncio.run(main()), ensure_ascii=False))
