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
| `@lat`, `@lon` | The latitude and the longitude of the feature in degrees. For several points, it is their mean position. For a line, it is the center of the line, with each part weighted by its length. For a polygon, it is the center of the area. |
| `@parts` | The number of parts of a line or a polygon |
| `@vertices` | The number of vertices of the feature. A ring that repeats its first vertex at the end counts it one time. |

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
| `a between x and y`, `a not between x and y` | True if `a` is at least `x` and at most `y` |
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
| `format_number(x, places)` | `x` as text with `places` decimal places, 0 if not given, and a comma between each three digits of the whole part. A half rounds away from zero, so `format_number(-1234.5)` gives `'-1,235'`. The places go from 0 to 15. |
| `round(x, digits)` | `x` rounded to `digits` decimal places, 0 if not given |
| `floor(x)` | The largest whole number not above `x` |
| `ceil(x)` | The smallest whole number not below `x` |
| `abs(x)` | The absolute value of `x` |
| `sqrt(x)` | The square root of `x`, or null if `x` is negative |
| `min(a, b, ...)` | The smallest number. Values that are not numbers are skipped. |
| `max(a, b, ...)` | The largest number. Values that are not numbers are skipped. |
| `clamp(low, x, high)` | `x`, but not below `low` and not above `high` |
| `area(unit)` | The area of a polygon. The unit is `'km2'`, `'mi2'`, `'ha'`, or `'acre'`, and `'km2'` if not given. |
| `perimeter(unit)` | The length of the edge of a polygon, holes included. The unit is `'km'`, `'mi'`, or `'m'`, and `'km'` if not given. |
| `line_length(unit)` | The length of a line, with the units of `perimeter`. The name `length` is the function for text. |
| `raster(layer, stat)` | The value of the raster layer named `layer` under the feature, in the unit of the layer. For a point, the value at the point. For a line or a polygon, `stat` is `'mean'`, `'min'`, or `'max'`, and `'mean'` if not given. A name that no raster layer has gives null, and the expression field shows a warning. |
| `flow(layer)` | The area of land that drains through a line, in km², from the height layer named `layer`. Each sample of the line moves to the nearest river cell within two cells of the coarse heightmap. The result is the largest area of the samples. A line far from all rivers gets a low flow. A point, a polygon, or a layer that is not a height layer gives null. |

The measures use the sphere of the world, with the radius of the world. The edges are great-circle arcs. A measure on the wrong geometry type gives null, for example the area of a point or `@parts` of a point. A wrong unit also gives null.

The filter `raster('Height') > 3000` matches the features above 3,000 m on the raster layer named Height.

With the Taper stroke, the width `clamp(0.5, sqrt(flow('Height')) / 20, 6)` gives a river that grows from its source to its mouth. The taper is narrow at the first vertex. The command "Orient rivers downhill" in the panel of a line layer puts the first vertex of each line at its upstream end.

### Page functions

A text item on a page of the atlas can use the page functions. In a filter, a style property, or a label text, a page function gives null, and the lists of functions do not show them.

```text
concat('Scale ', scale('fraction'))
concat('Page ', page_number(), ' of ', page_count())
```

| Function | Result |
| --- | --- |
| `scale(format, unit)` | The scale of the map frame. With `'fraction'`, the result is like `1:25,000,000`. With `'relative'`, it is a length on the page and the distance that it shows, like `1 cm = 250 km`. The unit is `'cm'` or `'in'`, and `'cm'` if not given. |
| `map_name()` | The name of the map in the map frame |
| `page_name()` | The name of the page |
| `page_number()` | The number of the page in the atlas. The first page is 1. |
| `page_count()` | The number of pages in the atlas |
| `date(format)` | The date of today. With `'long'`, the result is like `November 8, 2026`. With `'iso'`, it is like `2026-11-08`. |

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
                    | [ "not" ] ( "like" | "ilike" ) concat
                    | [ "not" ] "between" concat "and" concat ] ;
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

A name that is a keyword (`and`, `or`, `not`, `is`, `in`, `like`, `ilike`, `between`, `true`, `false`, `null`) is not a field. Put such a field name in double quotes.
