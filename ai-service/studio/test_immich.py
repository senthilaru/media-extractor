"""
Test and diagnose Immich connection, authentication, and person tagging.
Usage:
    python ai-service/studio/test_immich.py
"""

import os
import sys
from pathlib import Path

# Load project root and .env
current_dir = Path(__file__).resolve().parent
ai_service_dir = current_dir.parent
root_dir = ai_service_dir.parent

# Load environment variables from .env
dotenv_path = root_dir / ".env"
if dotenv_path.exists():
    with open(dotenv_path, "r") as f:
        for line in f:
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                k, v = line.split("=", 1)
                os.environ.setdefault(k.strip(), v.strip().strip('"').strip("'"))

if str(ai_service_dir) not in sys.path:
    sys.path.insert(0, str(ai_service_dir))

from studio.immich_client import ImmichClient


def main():
    print("=" * 60)
    print(" Immich Connection & Diagnostics")
    print("=" * 60)

    client = ImmichClient()
    print(f"Target URL:         {client.base_url}")
    print(f"Local Storage Dir:  {client.local_library_path}")

    try:
        status = client.test_connection()
        print(f"Server Status:      ONLINE (Version {status['version'].get('major')}.{status['version'].get('minor')}.{status['version'].get('patch')})")
        print(f"Local Path Exists:  {'YES' if status['local_storage_exists'] else 'NO'}")

        if status["authenticated"]:
            print(f"Authentication:     SUCCESS (User: {status['user']})")
            print("-" * 60)
            print("Fetching tagged people from Immich...")
            people = client.list_people()
            named_people = [p for p in people if p.get("name")]
            print(f"Total People Detected: {len(people)} (Named: {len(named_people)})")
            for p in named_people[:10]:
                print(f"  • {p.get('name')} (ID: {p.get('id')})")
            if len(named_people) > 10:
                print(f"  ... and {len(named_people) - 10} more.")
            print("-" * 60)
            print("Immich connection verified successfully!")
        else:
            print("Authentication:     NO API KEY CONFIGURED")
            print("-" * 60)
            print("To enable full queries, please generate an API key in Immich:")
            print("  1. Open http://localhost:2283 in your browser")
            print("  2. Click your user profile (top right) -> Account Settings -> API Keys")
            print("  3. Click 'New API Key', copy it, and add it to your .env file:")
            print('     IMMICH_API_KEY="your_copied_key"')
            print("-" * 60)
    except Exception as e:
        print(f"Error connecting to Immich: {e}")
        sys.exit(1)


if __name__ == "__main__":
    main()
