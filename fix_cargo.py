with open('scripts/start-max.ps1', 'r', encoding='utf-8') as f:
    content = f.read()

old_cargo = r'''        if ($name -eq "cargo.exe") {
            return ($cmd -like "*$repoRootLower*\core*") -or ($cmd -like "*$repoRootLower*\ui-dioxus*")
        }'''

new_cargo = r'''        if ($name -eq "trunk.exe") {
            return ($cmd -like "*$repoRootLower*\ui-rust-native*")
        }

        if ($name -eq "cargo.exe") {
            return ($cmd -like "*$repoRootLower*\core*") -or ($cmd -like "*$repoRootLower*\ui-rust-native*")
        }

        if ($name -eq "node.exe") {
            return ($cmd -like "*$repoRootLower*\ui-rust-native*")
        }'''

if old_cargo in content:
    content = content.replace(old_cargo, new_cargo)
    with open('scripts/start-max.ps1', 'w', encoding='utf-8') as f:
        f.write(content)
    print("Fixed cargo logic in start-max.ps1")
else:
    print("Could not find old_cargo in start-max.ps1")
