#!/usr/bin/env python3
"""wiki-text.py — print a mirrored wiki HTML page as plain text (headings, lists, table cells)."""
import sys, html.parser, re
class P(html.parser.HTMLParser):
    def __init__(s): super().__init__(); s.out=[]; s.skip=0
    def handle_starttag(s,t,a):
        if t in ('script','style'): s.skip+=1
        if t in ('p','br','li','tr','h1','h2','h3','h4','div','table'): s.out.append('\n')
        if t in ('h2','h3','h4'): s.out.append('## ')
        if t=='li': s.out.append('- ')
        if t in ('td','th'): s.out.append(' | ')
    def handle_endtag(s,t):
        if t in ('script','style'): s.skip-=1
    def handle_data(s,d):
        if not s.skip: s.out.append(d)
for f in sys.argv[1:]:
    p=P(); p.feed(open(f).read()); t=''.join(p.out)
    t=re.sub(r'\n\s*\n+', '\n', t); t=re.sub(r'[ \t]+',' ',t)
    print(f'=== {f}\n{t.strip()}\n')
