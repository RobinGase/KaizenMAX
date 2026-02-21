import argparse
import json
import urllib.error
import urllib.request


def main() -> None:
    parser = argparse.ArgumentParser(description="Upload a secret value to Kaizen vault")
    parser.add_argument("secret_name", help="Target secret name in /api/secrets/{secret_name}")
    parser.add_argument("value_path", help="Path to file whose content becomes secret value")
    parser.add_argument("--secret-type", default="opaque", help="Secret type label")
    parser.add_argument("--api-base", default="http://127.0.0.1:9100", help="Kaizen API base URL")
    parser.add_argument("--admin-token", default="", help="Optional x-admin-token header")
    args = parser.parse_args()

    with open(args.value_path, "r", encoding="utf-8") as f:
        text = f.read()

    payload = json.dumps({
        "value": text,
        "secret_type": args.secret_type,
    }).encode("utf-8")

    url = f"{args.api_base.rstrip('/')}/api/secrets/{args.secret_name}"
    req = urllib.request.Request(url, data=payload, method="PUT")
    req.add_header("Content-Type", "application/json")
    if args.admin_token:
        req.add_header("x-admin-token", args.admin_token)

    try:
        with urllib.request.urlopen(req) as response:
            print("Success:", response.status)
            print(response.read().decode("utf-8"))
    except urllib.error.HTTPError as e:
        print("HTTP error:", e.code)
        print(e.read().decode("utf-8"))
        raise
    except Exception as e:
        print("Error:", e)
        raise


if __name__ == "__main__":
    main()
