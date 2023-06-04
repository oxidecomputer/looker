# looker

A lens for removing glare and reducing eye strain while looking at Bunyan logs.

## Usage

See `looker --help` for usage options.

## Filtering with RHAI

The `-c` option accepts an [RHAI script](https://rhai.rs) that returns a Boolean
value indicating whether a record should be displayed. Each record is supplied
to the script in a variable named `r`.

The following Bunyan fields are guaranteed to exist for all records: `level`,
`name`, `hostname`, `pid`, `time`, and `msg`. Other fields, including
`component`, are optional and must be followed by the `?` operator for RHAI to
compile a script referring to them. Records that don't have a field referred to
in the script will be elided.

### Examples

- `looker -c 'r.msg.contains("Failed")'` - include all lines with a `msg` that
  contains `Failed`
- `looker -c 'r.response_code?.parse_int() >= 500'` - include all lines with a
  `response_code` field in the 5XX level
