-- 数据质量摘要。把 data.csv 换成真实文件，列名换成探测结果。
LOAD t FROM 'data.csv';

SELECT
  COUNT(*) AS n,
  COUNT(DISTINCT id) AS uniq_id,
  SUM(CASE WHEN id IS NULL THEN 1 ELSE 0 END) AS id_null,
  SUM(CASE WHEN TRIM(COALESCE(name, '')) = '' THEN 1 ELSE 0 END) AS name_blank
FROM t;
