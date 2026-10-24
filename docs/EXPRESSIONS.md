# Expressions

An expression is a short formula on the attributes of one feature. Filters, style properties, label text, and the attribute table use the same language. The language follows the expressions of QGIS.

```text
population > 50000 and kind = 'city'
if(capital, upper(name), name) || ' (' || to_text(population) || ')'
clamp(0.5, flow * 0.4, 6)
```

## Values

A value is a number, text, `true`, `false`, or `null`. Null means that no value is present.

| Write | For |
| --- | --- |
| `42`, `0.5`, `.5`, `1e6` | A number |
| `'Iron Hills'` | Text. Two single quotes stand for one quote: `'it''s'`. |
| `true`, `false`, `null` | The fixed values |
| `name` | The value of the field `name` |
| `"old name"` | The value of a field with a space or a keyword in its name. Two double quotes stand for one double quote. |
| `@scale` | The map scale denominator, for example `1000000` for 1:1,000,000. On the globe and the flat view, it is the scale at the center of the view. |
| `@geometry` | The geometry type of the feature: `'point'`, `'line'`, or `'polygon'` |

Field names are case-sensitive. Keywords, function names, and variable names are not case-sensitive.

## Operators

The table starts with the operators that bind most loosely.

| Operator | Result |
| --- | --- |
| `a or b` | True if one side is true |
| `a and b` | True if both sides are true |
| `not a` | The opposite of a true or false value |
| `=`, `!=`, `<>`, `<`, `<=`, `>`, `>=` | A comparison of two numbers, two texts, or two true or false values |
| `a is null`, `a is not null` | True if the value is null, or not null |
| `a in (x, y, ...)`, `a not in (...)` | True if `a` is equal to one of the items |
| `a like 'pattern'`, `a not like 'pattern'` | Text match. `%` matches any run of characters, and `_` matches one character. Case matters. |
| `a ilike 'pattern'` | The same as `like`, but case does not matter |
| `a \|\| b` | The two values joined as text |
| `+`, `-` | Addition and subtraction |
| `*`, `/`, `%` | Multiplication, division, and the remainder |
| `-a` | The negative of a number |

Parentheses change the order. A comparison takes one operator, so `1 < x < 3` is an error. Write `1 < x and x < 3`.

## Null and type rules

An operator on a null value gives null. A missing field is null. The exceptions are these:

- `false and null` is false, and `true or null` is true.
- `is null` and `is not null` always give true or false.
- `concat` and `coalesce` skip null values.

An operator on values of the wrong type gives null and not an error. Text does not change to a number in silence, so `'5' = 5` gives null. Use `to_number('5') = 5`. A division by zero gives null.

A filter matches a feature only if the result is `true`. The `if` function also takes only `true` as true.

## Functions

| Function | Result |
| --- | --- |
| `if(condition, a, b)` | `a` if the condition is true, else `b` |
| `coalesce(a, b, ...)` | The first value that is not null |
| `lower(text)` | The text in lower case |
| `upper(text)` | The text in upper case |
| `title(text)` | The text with an upper-case first letter in each word |
| `trim(text)` | The text without spaces at the start and the end |
| `length(text)` | The number of characters in the text |
| `substr(text, start, length)` | Part of the text. The first character is 1, and a negative start counts from the end. Without a length, the part goes to the end. |
| `left(text, n)` | The first `n` characters |
| `right(text, n)` | The last `n` characters |
| `replace(text, find, with)` | The text with each `find` replaced by `with` |
| `concat(a, b, ...)` | The values joined as text. Null values are skipped. |
| `to_text(value)` | The value as text. A whole number has no decimal point, so `to_text(5)` gives `'5'`. |
| `to_number(value)` | The value as a number, or null if the text is not a number. `true` is 1 and `false` is 0. |
| `round(x, digits)` | `x` rounded to `digits` decimal places, 0 if not given |
| `floor(x)` | The largest whole number not above `x` |
| `ceil(x)` | The smallest whole number not below `x` |
| `abs(x)` | The absolute value of `x` |
| `min(a, b, ...)` | The smallest number. Values that are not numbers are skipped. |
| `max(a, b, ...)` | The largest number. Values that are not numbers are skipped. |
| `clamp(low, x, high)` | `x`, but not below `low` and not above `high` |

## Errors

An error in the text gives a message with the line and the column, for example `1:8: unknown function 'shout'`. Unknown functions, unknown variables, and a wrong number of arguments are errors in the text. The nesting depth has a limit of 64 levels.

A wrong expression does not stop the map. A rule with a wrong filter draws no features. A property with a wrong expression, or with a result of the wrong type, uses its fixed value. A color property takes text in hex, for example `'#2f6f9f'`.

## Grammar

```text
expression = or ;
or         = and { "or" and } ;
and        = not { "and" not } ;
not        = "not" not | comparison ;
comparison = concat [ compare-op concat
                    | "is" [ "not" ] "null"
                    | [ "not" ] "in" "(" expression { "," expression } ")"
                    | [ "not" ] ( "like" | "ilike" ) concat ] ;
compare-op = "=" | "!=" | "<>" | "<" | "<=" | ">" | ">=" ;
concat     = sum { "||" sum } ;
sum        = term { ( "+" | "-" ) term } ;
term       = unary { ( "*" | "/" | "%" ) unary } ;
unary      = ( "-" | "+" ) unary | primary ;
primary    = number | text | "true" | "false" | "null"
           | field | variable | call | "(" expression ")" ;
call       = name "(" [ expression { "," expression } ] ")" ;
field      = name | '"' { character } '"' ;
variable   = "@" name ;
text       = "'" { character } "'" ;
number     = digits [ "." digits ] [ exponent ] | "." digits [ exponent ] ;
exponent   = ( "e" | "E" ) [ "+" | "-" ] digits ;
name       = ( letter | "_" ) { letter | digit | "_" } ;
```

A name that is a keyword (`and`, `or`, `not`, `is`, `in`, `like`, `ilike`, `true`, `false`, `null`) is not a field. Put such a field name in double quotes.
