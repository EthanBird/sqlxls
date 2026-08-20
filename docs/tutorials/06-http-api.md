# 06 · HTTP API

`http://` / `https://` 定位符**总是**走 HTTP 连接器。`format=` 只说明 body 怎么解码：`json` / `csv` / `excel`。

超时 30 秒。HTML 错误页会报错，不会当成 Excel。

## GET JSON

```sql
LOAD orders FROM 'https://api.example.com/orders' WITH (
  format='json',
  json_path='data'
);

SELECT status, COUNT(*) AS n
FROM orders
GROUP BY status;
```

## 鉴权

环境变量（推荐，不要写进仓库里的 SQL）：

```bash
export SQLXLS_BEARER_TOKEN='你的token'
# 未在 headers 里写 Authorization 时，自动加 Bearer
```

或自己拼 headers（JSON 对象字符串；`${TOKEN}` 从环境变量取，未定义则留空）：

```sql
LOAD t FROM 'https://api.example.com/orders' WITH (
  format='json',
  json_path='data',
  headers='{"Authorization":"Bearer ${TOKEN}","X-Request-Id":"sqlxls"}'
);
```

## POST

```sql
LOAD r FROM 'https://api.example.com/query' WITH (
  format='json',
  method='POST',
  body=read_text('payload.json'),
  headers='{"Content-Type":"application/json","Authorization":"Bearer ${TOKEN}"}',
  json_path='data.items'
);

SELECT * FROM r LIMIT 20;
```

`read_text` 是**标量**，只能嵌在参数里，不能 `FROM read_text(...)`。

`body` 若是本地文件路径，会按文件上传；否则当请求体字符串。

未写 Content-Type 且 body 非空时，默认 `application/json`。

## 分页（不要手写 page=1..n）

空页就停（`stop='empty'`，默认）：

```sql
LOAD items FROM 'https://api.example.com/items' WITH (
  format='json',
  json_path='data',
  page_param='page',
  page_from=1,
  page_to=100,
  page_size_param='limit',
  page_size=200
);

SELECT _page, COUNT(*) AS n FROM items GROUP BY _page;
```

偏移：

```sql
LOAD t FROM 'https://api.example.com/items' WITH (
  format='json',
  json_path='data',
  offset_param='offset',
  offset_step=100,
  page_to=50
);
```

来源列：`_page` 或 `_offset`。`stop='never'` 会跑满 `page_to`（即使中间已空）。

固定页数、接口从不会返回空页时，才用 `FOR page IN 1..12`。多数列表接口用 `page_param`。

cursor / `Link` 头分页尚未支持。

## 远程 CSV（含 GBK）

```sql
LOAD t FROM 'https://example.com/export.csv' WITH (
  format='csv',
  encoding='gbk'
);
```

若响应是 `text/csv; charset=gbk`，可省略 `encoding`。

## 远程 Excel 二进制

```sql
LOAD t FROM 'https://example.com/download' WITH (
  format='excel',
  sheet='Sheet1'
);
```

即使 Content-Type 是 `application/octet-stream`、URL 没有 `.xlsx`，只要文件头是 xlsx/xls 魔数就能读。详见 [03-excel.md](03-excel.md)。

## 按区域拉同一接口

```sql
SET base = 'https://api.example.com';

LOAD orders FROM '${base}/${region}/orders' WITH (
  format='json',
  json_path='data',
  page_param='page',
  page_to=50
)
FOR region IN ('east', 'west');

SELECT _region, status, COUNT(*) AS n
FROM orders
GROUP BY _region, status;
```

每个 region **自己**分页到空页，再 UNION。分析只写一次。更多：[07-dynamic-sources.md](07-dynamic-sources.md)。

## 下一步

[相似源：FOR / EACH / 日期](07-dynamic-sources.md)
