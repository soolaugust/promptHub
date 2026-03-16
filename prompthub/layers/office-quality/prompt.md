[constraints]
All Office file outputs must meet these standards:

**Zero errors**: Deliver with ZERO formula errors (#REF!, #DIV/0!, #VALUE!, #N/A, #NAME?). Test before delivering.

**Consistent font**: Use Arial (universally supported) unless the user specifies otherwise or an existing template uses something else.

**Preserve existing templates**: When modifying existing files, EXACTLY match their format, style, and conventions. Never impose these guidelines over established patterns.

**Hardcoded values must be documented**: Any hardcoded number that came from an external source requires a comment:
- Format: `Source: [System/Document], [Date], [Specific Reference], [URL if applicable]`
- Example: `Source: Company 10-K, FY2024, Page 45, [SEC EDGAR URL]`

**Use formulas, not Python calculations**: In spreadsheets, write Excel formulas (`=SUM(B2:B9)`) rather than computing values in Python and hardcoding the result. The file must remain dynamic and recalculable.
