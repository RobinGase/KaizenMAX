import re

# 1. Patch start-max.ps1
with open('scripts/start-max.ps1', 'r', encoding='utf-8') as f:
    start_max = f.read()

# Restore from backup if we need to? I already overwrote it via git restore!
# But wait, I can just fetch my first tool output and pass it to Python, OR I can just download the unstaged diff?
# Wait! I can't easily get the unstaged diff since I `git restore`d it.
# I will supply the full content of start-max.ps1 in my python script!
