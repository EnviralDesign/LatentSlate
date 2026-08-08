from pathlib import Path
import re


path = Path("scripts/provider_identity_one_shot.py")
text = path.read_text()

old = '''replace_once(
    "src/egui_app/provider_builder.rs",
    ''' + "'''" + '''        Pos2::new(rect.left(), rect.bottom() - 2.0),
    ''' + "'''" + ''',
    ''' + "'''" + '''        Pos2::new(text_left, rect.bottom() - 2.0),
    ''' + "'''" + ''',
)
'''
new = '''replace_once(
    "src/egui_app/provider_builder.rs",
    ''' + "'''" + '''    paint_truncated_row_text_bottom(
        ui,
        Pos2::new(rect.left(), rect.bottom() - 2.0),
        kit::caption(&summary.subtitle),
    ''' + "'''" + ''',
    ''' + "'''" + '''    paint_truncated_row_text_bottom(
        ui,
        Pos2::new(text_left, rect.bottom() - 2.0),
        kit::caption(&summary.subtitle),
    ''' + "'''" + ''',
)
'''
if text.count(old) != 1:
    raise SystemExit(f"Expected one ambiguous provider-row selector, found {text.count(old)}")
text = text.replace(old, new, 1)

text, docs_count = re.subn(
    r'\nreplace_once\(\n    "docs/PROVIDERS\.md",.*?\n\)\n?\Z',
    '\n',
    text,
    count=1,
    flags=re.S,
)
if docs_count != 1:
    raise SystemExit(f"Expected one optional docs edit block, found {docs_count}")

path.write_text(text)
