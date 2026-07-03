with open('refactor.py', 'r') as f:
    code = f.read()

code = code.replace(
    '''code = code.replace(
"if (accounts.length === 0) {",
"const accounts = getAccounts();\\n  if (accounts.length === 0) {")''',
    '''code = code.replace(
"  if (accounts.length === 0) {\\n    customAlert('请先在账号管理中添加账号');",
"  const accounts = getAccounts();\\n  if (accounts.length === 0) {\\n    customAlert('请先在账号管理中添加账号');")'''
)

with open('refactor.py', 'w') as f:
    f.write(code)

