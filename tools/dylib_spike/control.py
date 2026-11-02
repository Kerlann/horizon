#!/usr/bin/env python3
"""Use the game's existing line/JSON control protocol without building horizon-ctl."""
import json
import socket
import sys

with socket.socket(socket.AF_UNIX) as connection:
    connection.settimeout(180)
    connection.connect(sys.argv[1])
    connection.sendall((" ".join(sys.argv[2:]) + "\n").encode())
    reply = connection.makefile().readline()
    print(json.dumps(json.loads(reply), indent=2))
